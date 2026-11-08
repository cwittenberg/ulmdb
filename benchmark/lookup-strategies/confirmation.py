#!/usr/bin/env python3
"""Confirm the entire regression matrix using the primary campaign's binaries.

No compilation, workload selection, LMDB-control normalization or primary
artifact replacement occurs. All primary observations remain separate.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import regression as reg
from provenance import environment, machine, sha


def check_configuration(primary, args):
    reg.require(primary.get("complete") is True, "primary campaign must be complete")
    reg.require(args.cpu == primary["configuration"]["cpu"], "confirmation CPU differs from primary")
    reg.require(args.rounds >= 3, "at least three fresh confirmation pairs required")
    reg.require(args.cpu in os.sched_getaffinity(0), "CPU is outside allowed affinity")
    reg.require(primary["backend"] == primary["configuration"]["backend"], "primary backend configuration differs")
    reg.require(set(primary["sources"]) == set(reg.ARMS) and set(primary["binaries"]) == set(reg.ARMS),
                "primary arm inventory differs")
    reg.require(primary["schedule"] == reg.schedule(primary["configuration"]["rounds"]),
                "primary schedule differs")
    reg.require(len(primary["runs"]) == len(primary["schedule"]), "primary has missing processes")
    reg.require([{key: run[key] for key in ("pair", "filesystem", "arm")} for run in primary["runs"]]
                == primary["schedule"], "primary completed process identities differ")


def check_inputs(primary):
    for arm in reg.ARMS:
        source = Path(primary["sources"][arm]["directory"])
        reg.verify_source(source, primary["sources"][arm]["sha256"])
        reg.require(sha(Path(primary["binaries"][arm]["path"])) == primary["binaries"][arm]["sha256"],
                    f"primary {arm} binary changed")


def verify_primary_files(primary_dir, expected):
    for relative, digest in expected.items():
        reg.require(sha(primary_dir / relative) == digest, f"primary artifact changed: {relative}")


def primary_delta(output, primary_table, confirmation_table):
    old = {(row["filesystem"], row["case"]): row for row in primary_table["throughput_cases"]}
    new = {(row["filesystem"], row["case"]): row for row in confirmation_table["throughput_cases"]}
    reg.require(set(old) == set(new) and len(old) == 30, "primary/confirmation case inventories differ")
    rows = [{"filesystem": key[0], "case": key[1], "primary": old[key], "confirmation": new[key],
             "throughput_change_delta_percentage_points": new[key]["ulmdb_change_pct"] - old[key]["ulmdb_change_pct"],
             "sampled_p99_change_delta_percentage_points": new[key]["sampled_p99_change_pct"] - old[key]["sampled_p99_change_pct"]}
            for key in sorted(old)]
    reg.write_json(output / "PRIMARY_VS_CONFIRMATION.json", {"schema": 1, "rows": rows,
                   "scope": "separate complete campaigns; primary retained, no pooling, selection or LMDB normalization"})
    text = ["# Primary and confirmation observations", "",
            "Both complete 30-case campaigns are retained separately. Changes compare candidate with baseline within each campaign. Delta is confirmation change minus primary change in percentage points. All sampled latency changes and native LMDB control drift remain visible. Neither campaign establishes universal nonregression.", "",
            "| Filesystem | Case | Primary ULMDB throughput change | Confirmation throughput change | Delta pp | Primary sampled p99 change | Confirmation sampled p99 change | Primary LMDB drift | Confirmation LMDB drift |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    for row in rows:
        a, b = row["primary"], row["confirmation"]
        text.append(f"| {row['filesystem']} | {row['case']} | {a['ulmdb_change_pct']:+.2f}% | {b['ulmdb_change_pct']:+.2f}% | {row['throughput_change_delta_percentage_points']:+.2f} | {a['sampled_p99_change_pct']:+.2f}% | {b['sampled_p99_change_pct']:+.2f}% | {a['lmdb_control_change_pct']:+.2f}% | {b['lmdb_control_change_pct']:+.2f}% |")
    (output / "PRIMARY_VS_CONFIRMATION.md").write_text("\n".join(text) + "\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--primary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new private evidence directory")
    parser.add_argument("--cpu", type=int, default=12)
    parser.add_argument("--rounds", type=int, default=9)
    parser.add_argument("--keep-fixtures", action="store_true")
    parser.add_argument("--validate-only", action="store_true", help="verify primary inputs and raw samples without creating output or starting measurements")
    args = parser.parse_args(argv)
    primary_dir, output = args.primary.resolve(), args.output.resolve()
    primary = json.loads((primary_dir / "results.json").read_text())
    check_configuration(primary, args)
    reg.require(not output.exists() and output != primary_dir, "confirmation output must be new")
    check_inputs(primary)
    expected_primary = {name: sha(primary_dir / name) for name in (
        "results.json", "provenance.json", "baseline.full.json", "candidate.full.json", "comparison.json",
        "table.json", "REGRESSION.md")}
    for run in primary["runs"]:
        for key, hash_key in (("raw_file", "raw_sha256"), ("latency_file", "latency_sha256")):
            reg.require(sha(primary_dir / run[key]) == run[hash_key], "primary raw artifact changed")
            expected_primary[run[key]] = run[hash_key]
    primary_captures = {arm: json.loads((primary_dir / f"{arm}.full.json").read_text()) for arm in reg.ARMS}
    primary_table = json.loads((primary_dir / "table.json").read_text())
    signature = None
    native_identity = None
    for arm, capture in primary_captures.items():
        reg.require(len(capture["profiles"]) == 2 * primary["configuration"]["rounds"], "primary capture process count differs")
        for profile in capture["profiles"]:
            raw_file = primary_dir / profile["raw_file"]
            reg.require(sha(raw_file) == profile["raw_sha256"], "primary profile raw report changed")
            reg.require(json.loads(raw_file.read_text()) == profile["report"], "primary raw report differs from capture")
            signature = reg.validate_report(profile["report"], (primary_dir / profile["latency_copy"]).read_bytes(),
                       primary["binaries"][arm]["sha256"], primary["sources"][arm]["sha256"], primary["backend"], signature)
            identity = {"version": profile["report"]["native_version"], "flags": profile["report"]["native_flags"]}
            reg.require(native_identity is None or native_identity == identity, "primary native reference differs")
            native_identity = identity
    compare_path = primary_dir / "tooling/compare.py"
    reg.require(sha(compare_path) == primary["tooling_sha256"]["compare.py"], "primary comparison tooling changed")
    compare = reg.import_compare(compare_path)
    current_machine = machine(args.cpu)
    for key in ("cpu_model", "architecture", "kernel", "rustc"):
        reg.require(current_machine[key] == primary["machine"][key], f"confirmation machine/compiler differs: {key}")
    if args.validate_only:
        print("Primary source, executable, configuration, workload and raw-sample checks passed; no measurements started.")
        return 0
    output.mkdir(parents=True, exist_ok=False)
    (output / "raw").mkdir()
    (output / "tooling").mkdir()
    tooling = {"confirmation.py": Path(__file__), "regression.py": Path(reg.__file__),
               "provenance.py": Path(reg.__file__).with_name("provenance.py"), "compare.py": compare_path}
    for name, path in tooling.items():
        shutil.copyfile(path, output / "tooling" / name)
    receipt = {"schema": 1, "complete": False, "campaign": "full-matrix confirmation",
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "configuration": {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()},
               "primary_directory": str(primary_dir), "primary_artifacts_sha256": expected_primary,
               "sources": primary["sources"], "binaries": primary["binaries"], "backend": primary["backend"],
               "machine": current_machine, "schedule": reg.schedule(args.rounds), "runs": [],
               "tooling_sha256": {name: sha(output / "tooling" / name) for name in tooling},
               "scope": "new full-matrix observations over the exact primary binaries; primary retained separately; no recompilation or LMDB normalization"}
    reg.write_json(output / "provenance.json", receipt)
    disk = output / "fixtures/disk"
    disk.mkdir(parents=True)
    ram = Path(tempfile.mkdtemp(prefix="ulmdb-lookup-confirmation-", dir=primary["configuration"]["ram_dir"]))
    media = {"disk": disk, "ram": ram}
    media_info = {key: reg.filesystem(path) for key, path in media.items()}
    for medium in media:
        reg.require(media_info[medium] == primary_captures["baseline"]["filesystems"][medium],
                    f"confirmation filesystem/device differs: {medium}")
    receipt["fixture_roots"] = {key: str(value) for key, value in media.items()}
    captures = {arm: {"schema": 1, "suite": "full", "backend": primary["backend"], "rounds": args.rounds,
                    "generated_at": receipt["started_at"], "machine": current_machine, "cpu": args.cpu,
                    "filesystems": media_info, "source_sha256": primary["sources"][arm]["sha256"], "profiles": []}
                for arm in reg.ARMS}
    env = environment() | {"PYTHONDONTWRITEBYTECODE": "1"}
    try:
        for job in receipt["schedule"]:
            check_inputs(primary)
            arm, medium, pair = job["arm"], job["filesystem"], job["pair"]
            stem = f"{medium}-p{pair}-{arm}"
            directory = media[medium] / stem
            raw_out = output / f"{stem}.json"
            stderr = output / f"{stem}.stderr.log"
            print(f"Confirming {stem}", flush=True)
            with raw_out.open("w") as out, stderr.open("w") as err:
                subprocess.run(["taskset", "-c", str(args.cpu), primary["binaries"][arm]["path"], "bench", str(directory)],
                               cwd=primary["sources"][arm]["directory"], env=env, stdout=out, stderr=err, check=True)
            report = json.loads(raw_out.read_text())
            reg.require(report["raw_latency_file"] == "latencies.u64le", "unexpected latency filename")
            latency_copy = output / "raw" / f"{stem}.latencies.u64le"
            shutil.copyfile(directory / "latencies.u64le", latency_copy)
            reg.validate_report(report, latency_copy.read_bytes(), primary["binaries"][arm]["sha256"],
                                primary["sources"][arm]["sha256"], primary["backend"], signature)
            reg.require({"version": report["native_version"], "flags": report["native_flags"]} == native_identity,
                        "confirmation native LMDB identity differs")
            reg.require(report["filesystem"].strip() == media_info[medium]["type"], "actual confirmation filesystem differs")
            check_inputs(primary)
            captures[arm]["profiles"].append({"filesystem": medium, "round": pair, "pair": pair,
                "raw_file": raw_out.name, "raw_sha256": sha(raw_out),
                "latency_copy": str(latency_copy.relative_to(output)), "report": report})
            receipt["runs"].append(job | {"raw_file": raw_out.name, "raw_sha256": sha(raw_out),
                "latency_file": str(latency_copy.relative_to(output)), "latency_sha256": sha(latency_copy)})
            reg.write_json(output / f"{arm}.partial.json", captures[arm])
            reg.write_json(output / "provenance.json", receipt)
            if not args.keep_fixtures:
                shutil.rmtree(directory)
        for arm in reg.ARMS:
            reg.require(len(captures[arm]["profiles"]) == 2 * args.rounds, "missing confirmation process")
            reg.write_json(output / f"{arm}.full.json", captures[arm])
        result = compare.compare({"full": captures["baseline"]}, {"full": captures["candidate"]},
                                 paired=True, min_repeats=3, adjustment="bonferroni")
        reg.write_json(output / "comparison.json", result)
        (output / "COMPARISON.md").write_text(compare.markdown(result))
        reg.render(output, captures, result)
        primary_delta(output, primary_table, json.loads((output / "table.json").read_text()))
        check_inputs(primary)
        verify_primary_files(primary_dir, expected_primary)
        receipt["complete"] = True
        receipt["screen_clear"] = result["summary"]["screen_clear"]
        receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        reg.write_json(output / "results.json", receipt)
        reg.write_json(output / "provenance.json", receipt)
        if not args.keep_fixtures:
            ram.rmdir()
        print(f"Confirmation evidence: {output / 'REGRESSION.md'}", flush=True)
        return 0
    except Exception as error:
        receipt["failure"] = {"type": type(error).__name__, "message": str(error)}
        reg.write_json(output / "failure.json", receipt)
        raise


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Confirmation failed; retained diagnostics: {error}", file=sys.stderr)
        sys.exit(2)
