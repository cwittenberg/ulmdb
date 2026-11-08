#!/usr/bin/env python3
"""Run interleaved, real C adapter journeys against two frozen shared libraries.

This bounded Linux Legacy-adapter screen is separate from benchmark/ and never
rewrites its reports. Retain every database, sample, failed process and hash.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shlex
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
M0_HASH = "a40923a5446fbea259aec44c2394886b691004e3d323283a7185666b54c6c3e8"
PHASES = ("create_open", "initial_load", "batched_get", "single_get_transaction",
          "update_batch1", "update_batch16", "close_reopen")


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def filesystem(path):
    return subprocess.run(["stat", "-f", "-c", "%T", str(path)], check=True,
                          text=True, capture_output=True).stdout.strip()


def physical(path):
    entries = [path, *path.rglob("*")]
    require(not any(item.is_symlink() for item in entries), "unexpected database symlink")
    return {"apparent_bytes": sum(item.stat().st_size for item in entries if item.is_file()),
            "allocated_bytes": sum(item.stat().st_blocks * 512 for item in entries),
            "files": sum(item.is_file() for item in entries)}


def percentile(samples, q):
    return sorted(samples)[max(0, math.ceil(len(samples) * q) - 1)]


def metrics(raw):
    require(raw.get("schema") == 1 and raw.get("passed") is True, "journey did not pass")
    require(tuple(row["phase"] for row in raw["metrics"]) == PHASES, "journey phase mismatch")
    result = {}
    for row in raw["metrics"]:
        samples = row["latency_ns"]
        require(samples and all(isinstance(n, int) and n > 0 for n in samples), "invalid latency samples")
        require(row["elapsed_ns"] > 0 and row["operations"] > 0, "invalid phase work")
        result[row["phase"]] = {
            "operations_per_second": row["operations"] * 1e9 / row["elapsed_ns"],
            "elapsed_ns": row["elapsed_ns"], "transactions": row["transactions"],
            "sample_unit": row["sample_unit"], "sample_count": len(samples),
            "p50_ns": percentile(samples, .5), "p95_ns": percentile(samples, .95),
            "p99_ns": percentile(samples, .99)}
    return result


def run_one(args, executable, library, database, directory):
    directory.mkdir(parents=True)
    command = [str(executable), str(database), str(args.keys), str(args.batched_reads),
               str(args.single_reads), str(args.write_transactions), str(args.reopens)]
    env = os.environ.copy()
    for name in ("LD_PRELOAD", "ULMDB_LIBRARY", "ULMDB_MIGRATOR"):
        env.pop(name, None)
    env.update(LD_LIBRARY_PATH=str(library.parent), LD_BIND_NOW="1", ULMDB_LMDB_ABI="0.9")
    started = time.monotonic()
    peak, observations, unavailable, timed_out = 0, 0, 0, False
    with (directory / "stdout.json").open("w") as stdout, (directory / "stderr.log").open("w") as stderr:
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr, env=env)
        while process.poll() is None:
            try:
                peak = max(peak, len(os.listdir(f"/proc/{process.pid}/fd")))
                observations += 1
            except (FileNotFoundError, PermissionError, ProcessLookupError):
                # Linux can deny procfs briefly during exec or process exit.
                # Record missing observations rather than failing an engine.
                unavailable += 1
            if time.monotonic() - started > args.timeout:
                timed_out = True
                process.kill()
            time.sleep(.001)
        code = process.wait()
    record = {"command": command, "exit_code": code, "timed_out": timed_out,
              "process_seconds": time.monotonic() - started, "database": str(database),
              "stdout": str(directory / "stdout.json"), "stderr": str(directory / "stderr.log"),
              "fd_peak_observed": peak, "fd_observations": observations,
              "fd_unavailable_observations": unavailable,
              "passed": False}
    try:
        require(code == 0 and not timed_out, f"journey exited {code}")
        require(observations > 0, "FD monitor obtained no readable observations")
        raw = json.loads((directory / "stdout.json").read_text())
        require(Path(raw["provider"]).resolve() == library.resolve(), "loaded library differs from selected artifact")
        require(raw["keys"] == args.keys and raw["value_bytes"] == 64, "workload differs")
        record.update(metrics=metrics(raw), checksums=raw["checksums"], physical=physical(database), passed=True)
    except (RuntimeError, ValueError, KeyError, OSError) as error:
        record["error"] = str(error)
    write_json(directory / "run.json", record)
    return record


def aggregate(runs):
    comparisons = []
    for medium in sorted({row["filesystem"] for row in runs}):
        selected = [row for row in runs if row["filesystem"] == medium and row["passed"]]
        arms = {arm: [row for row in selected if row["arm"] == arm] for arm in ("baseline", "candidate")}
        if not all(arms.values()):
            continue
        for phase in PHASES:
            summary = {"filesystem": medium, "phase": phase, "arms": {}}
            for arm, rows in arms.items():
                raw_samples = []
                for row in rows:
                    raw = json.loads(Path(row["stdout"]).read_text())
                    raw_samples.extend(next(item["latency_ns"] for item in raw["metrics"] if item["phase"] == phase))
                rates = [row["metrics"][phase]["operations_per_second"] for row in rows]
                summary["arms"][arm] = {
                    "processes": len(rows), "mean_operations_per_second": statistics.mean(rates),
                    "sample_unit": rows[0]["metrics"][phase]["sample_unit"], "pooled_samples": len(raw_samples),
                    **{f"pooled_p{q}_ns": percentile(raw_samples, q/100) for q in (50,95,99)}}
            pairs, tails = [], []
            for pair in sorted({row["pair"] for row in selected}):
                match = {row["arm"]: row for row in selected if row["pair"] == pair}
                if len(match) == 2:
                    pairs.append((match["candidate"]["metrics"][phase]["operations_per_second"] /
                                  match["baseline"]["metrics"][phase]["operations_per_second"] - 1) * 100)
                    tails.append((match["candidate"]["metrics"][phase]["p99_ns"] /
                                  match["baseline"]["metrics"][phase]["p99_ns"] - 1) * 100)
            summary["paired_throughput_changes_percent"] = pairs
            summary["mean_paired_change_percent"] = statistics.mean(pairs) if pairs else None
            summary["adverse_screen_flag"] = bool(pairs and all(change < 0 for change in pairs)
                                                  and statistics.mean(pairs) <= -5)
            summary["paired_p99_changes_percent"] = tails
            summary["mean_paired_p99_change_percent"] = statistics.mean(tails) if tails else None
            summary["latency_review_flag"] = bool(tails and all(change > 0 for change in tails)
                                                  and statistics.mean(tails) >= 10)
            comparisons.append(summary)
    return comparisons


def report(receipt):
    lines = ["# Legacy C adapter before/after performance screen", "",
             f"Complete: **{receipt['complete']}**; logical oracles: **{receipt['passed']}**.",
             "This bounded profile does not qualify Resident, Range, native mixed-process maintenance or general performance parity.",
             "This compares exact artifacts; unrelated source changes may differ, so it does not establish a causal M1 effect.",
             "", "Throughput includes public C API transaction work and deterministic value checks. Updates include durable commits. No durability flags are weakened.",
             "Latency sample units are explicit below; initial-load latency has one sample per process. p99 with a small sample count is a sampled maximum, not a stable tail estimate.",
             "Fresh databases/processes use the OS's current cache state. An external 1 ms procfs monitor records a lower bound on FD peaks; monitoring overhead affects both arms.",
             "", "| Filesystem / phase | Baseline ops/s | Candidate ops/s | Mean paired change | Pairs | Throughput flag | P99 flag |",
             "| --- | ---: | ---: | ---: | --- | --- | --- |"]
    for row in receipt["comparisons"]:
        a, b = row["arms"]["baseline"], row["arms"]["candidate"]
        change = f"{row['mean_paired_change_percent']:+.2f}%" if row['mean_paired_change_percent'] is not None else "-"
        lines.append(f"| {row['filesystem']} / {row['phase']} | {a['mean_operations_per_second']:,.1f} | {b['mean_operations_per_second']:,.1f} | {change} | " + ", ".join(f"{v:+.2f}%" for v in row["paired_throughput_changes_percent"]) + f" | {row['adverse_screen_flag']} | {row['latency_review_flag']} |")
    lines += ["", "A throughput flag means all observed pairs slowed and mean throughput fell at least 5%. A P99 flag means every paired process p99 rose, averaging at least 10%. These are review screens, not acceptance rules or statistical significance claims.",
              "", "| Filesystem / phase / arm | Sample unit | Samples | Pooled p50 ns | Pooled p95 ns | Pooled p99 ns |",
              "| --- | --- | ---: | ---: | ---: | ---: |"]
    for row in receipt["comparisons"]:
        for arm, data in row["arms"].items():
            lines.append(f"| {row['filesystem']} / {row['phase']} / {arm} | {data['sample_unit']} | {data['pooled_samples']} | {data['pooled_p50_ns']:,} | {data['pooled_p95_ns']:,} | {data['pooled_p99_ns']:,} |")
    lines += ["", "| Filesystem / pair / arm | Apparent bytes | Allocated bytes, including directories | Files | FD peak observed | Exit |",
              "| --- | ---: | ---: | ---: | ---: | ---: |"]
    for row in receipt["runs"]:
        size = row.get("physical", {})
        lines.append(f"| {row['filesystem']} / {row['pair']} / {row['arm']} | {size.get('apparent_bytes','-')} | {size.get('allocated_bytes','-')} | {size.get('files','-')} | {row['fd_peak_observed']} | {row['exit_code']} |")
    lines += ["", "Artifact hashes, every process, raw latency samples, checksums, failures and preserved database paths are recorded in [receipt.json](receipt.json).", ""]
    if "noise_control" in receipt:
        lines += [f"Identical-artifact noise control: `{receipt['noise_control']['path']}` "
                  f"(SHA-256 `{receipt['noise_control']['sha256']}`). Its outliers and review flags remain evidence of measurement noise.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, default=ROOT / "clients/lmdb-ulmdb/target/relative-output-check/lib/liblmdb.so.0")
    parser.add_argument("--baseline-sha256", default=M0_HASH)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--candidate-sha256", help="Require this exact candidate artifact before capture")
    parser.add_argument("--binary", type=Path, help="Use one already compiled journey executable")
    parser.add_argument("--control-receipt", type=Path, help="Attach a passing identical-baseline noise control")
    parser.add_argument("--header-dir", type=Path, default=Path.home()/".cache/ulmdb-compat/lmdb-0.9.31")
    parser.add_argument("--reference-lib", type=Path, default=ROOT/"target/qualification/reference-0.9.31/lib")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--disk-root", type=Path, help="Default: a new disk-data directory inside output")
    parser.add_argument("--ram-root", type=Path, default=Path("/dev/shm"))
    parser.add_argument("--pairs", type=int, default=3)
    parser.add_argument("--keys", type=int, default=256)
    parser.add_argument("--batched-reads", type=int, default=4096)
    parser.add_argument("--single-reads", type=int, default=32)
    parser.add_argument("--write-transactions", type=int, default=32)
    parser.add_argument("--reopens", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=180)
    args = parser.parse_args()
    require(platform.system() == "Linux", "this harness requires Linux")
    require(args.pairs >= 3, "at least three interleaved pairs are required")
    require(args.timeout > 0, "timeout must be positive")
    for field in ("keys", "batched_reads", "single_reads", "write_transactions", "reopens"):
        require(getattr(args, field) > 0, f"{field} must be positive")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    receipt = {"schema": 1, "scope": "Linux Legacy C adapter, local bounded journey",
               "started_utc": datetime.now(timezone.utc).isoformat(), "complete": False,
               "passed": False, "performance_acceptance": "requires review", "runs": [], "comparisons": []}
    try:
        receipt["harness_sha256"] = {str(path): sha(path) for path in (HERE/"performance_journey.c", Path(__file__))}
        require(sha(args.baseline.resolve()) == args.baseline_sha256, "baseline artifact does not match expected qualified hash")
        libraries = {}
        for arm, original in (("baseline", args.baseline), ("candidate", args.candidate)):
            destination = args.output / "libraries" / arm / "liblmdb.so.0"
            destination.parent.mkdir(parents=True)
            before = sha(original.resolve())
            expected = args.baseline_sha256 if arm == "baseline" else args.candidate_sha256
            require(expected is None or before == expected, f"{arm} differs from the requested artifact hash")
            shutil.copy2(original.resolve(), destination)
            require(sha(original.resolve()) == sha(destination) == before, f"{arm} changed during capture")
            libraries[arm] = destination
        receipt["artifacts"] = {arm: {"original": str(path.resolve()), "sha256": sha(libraries[arm])}
                                for arm, path in (("baseline", args.baseline), ("candidate", args.candidate))}
        receipt["configuration"] = {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()}
        receipt["host"] = {"platform": platform.platform(), "processor": platform.processor(), "cpu_count": os.cpu_count()}
        executable = args.output / "journey"
        if args.binary:
            shutil.copy2(args.binary.resolve(), executable)
            receipt["binary_origin"] = str(args.binary.resolve())
        else:
            pins = json.loads((ROOT/"clients/lmdb-ulmdb/legacy-source.json").read_text())
            expected_header = next(row["sha256"] for row in pins["files"] if row["path"] == "lmdb.h")
            require(sha(args.header_dir / "lmdb.h") == expected_header, "header differs from pinned LMDB0.9.31")
            compiler = shlex.split(os.environ.get("CC", "cc"))
            command = compiler + ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-pthread",
                                  "-I", str(args.header_dir.resolve()), str(HERE/"performance_journey.c"),
                                  "-L", str(args.reference_lib.resolve()), "-llmdb", "-ldl", "-o", str(executable)]
            result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (args.output / "compile.log").write_text(result.stdout)
            require(result.returncode == 0, "journey compilation failed; see compile.log")
            receipt["compile"] = {"command": command, "source_sha256": sha(HERE/"performance_journey.c"),
                                  "header_sha256": expected_header,
                                  "reference_library_sha256": sha(args.reference_lib / "liblmdb.so.0")}
            receipt["compile"]["compiler"] = subprocess.run(compiler + ["--version"], check=True,
                text=True, capture_output=True).stdout.splitlines()[0]
        receipt["binary_sha256"] = sha(executable)
        if args.control_receipt:
            control_path = args.control_receipt.resolve()
            control = json.loads(control_path.read_text())
            require(control.get("complete") and control.get("passed"), "noise control did not pass")
            require(all(control["artifacts"][arm]["sha256"] == args.baseline_sha256
                        for arm in ("baseline", "candidate")), "noise control used different artifacts")
            require(control["binary_sha256"] == receipt["binary_sha256"], "noise control used different application bytes")
            for key in ("keys", "batched_reads", "single_reads", "write_transactions", "reopens"):
                require(control["configuration"][key] == getattr(args,key), "noise control used different work")
            receipt["noise_control"] = {"path": str(control_path), "sha256": sha(control_path),
                "comparisons": control["comparisons"]}
        disk = (args.disk_root.resolve() if args.disk_root else args.output/"disk-data")
        disk.mkdir(parents=True, exist_ok=True)
        require(filesystem(disk) not in ("tmpfs", "ramfs", "nfs", "nfs4", "cifs", "smb2"), "disk root is not local disk storage")
        media = [("disk", disk)]
        if args.ram_root.is_dir() and filesystem(args.ram_root) in ("tmpfs", "ramfs"):
            ram = args.ram_root.resolve() / f"ulmdb-c-performance-{os.getpid()}-{time.time_ns()}"
            ram.mkdir()
            media.append(("ram", ram))
        else:
            receipt["ram_unavailable"] = "requested RAM root is not an available tmpfs/ramfs mount"
        receipt["filesystems"] = {label: {"root": str(path), "type": filesystem(path)} for label,path in media}
        write_json(args.output/"receipt.json", receipt)
        for pair in range(args.pairs):
            for medium, root in media:
                order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
                for arm in order:
                    name = f"{medium}-{pair+1:02}-{arm}"
                    row = run_one(args, executable, libraries[arm], root/name,
                                  args.output/"runs"/name)
                    row.update(filesystem=medium, pair=pair+1, arm=arm)
                    receipt["runs"].append(row)
                    write_json(args.output/"receipt.json", receipt)
                    print(f"{'PASS' if row['passed'] else 'FAIL'}: {name} ({row['process_seconds']:.3f}s)", flush=True)
        passed = all(row["passed"] for row in receipt["runs"])
        images = {json.dumps(row["checksums"], sort_keys=True) for row in receipt["runs"] if row["passed"]}
        require(len(images) == 1, "logical checksum differs across libraries/processes/filesystems")
        require(sha(executable) == receipt["binary_sha256"], "journey executable changed")
        for arm,path in libraries.items():
            require(sha(path) == receipt["artifacts"][arm]["sha256"], "frozen library changed")
        require(all(sha(Path(path)) == value for path,value in receipt["harness_sha256"].items()),
                "performance harness changed during measurement")
        receipt.update(complete=True, passed=passed, comparisons=aggregate(receipt["runs"]))
    except Exception as error:
        receipt["error"] = str(error)
    receipt["finished_utc"] = datetime.now(timezone.utc).isoformat()
    write_json(args.output/"receipt.json", receipt)
    (args.output/"REPORT.md").write_text(report(receipt))
    print(f"{'PASS' if receipt['passed'] else 'FAIL'}: oracles; evidence {args.output}", flush=True)
    return 0 if receipt["complete"] and receipt["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
