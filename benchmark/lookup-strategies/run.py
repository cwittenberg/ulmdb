"""Run isolated lookup experiments; existing public benchmark reports are untouched."""
from pathlib import Path
import argparse
import csv
import datetime
import io
import json
import os
import shutil
import subprocess

from provenance import environment, freeze, machine, sha, source_files, verify_frozen
from validate import validate

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
EXECUTABLES = {"prefix": "ulmdb-prefix-strategy-benchmark",
               "exact": "ulmdb-exact-strategy-benchmark",
               "ordered": "ulmdb-ordered-lookup-benchmark"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["all", *EXECUTABLES], default="all")
    parser.add_argument("--sizes", default="8192,100000,1000000")
    parser.add_argument("--queries", type=int, default=65536)
    parser.add_argument("--prefix-iterations", type=int, default=1)
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--processes", type=int, default=3)
    parser.add_argument("--cpu", type=int, default=20)
    parser.add_argument("--seed", type=int, default=73)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--resume", action="store_true", help="retain completed samples and rebuild corrected harnesses over the same frozen engine")
    args = parser.parse_args()
    sizes = [int(x) for x in args.sizes.split(",")]
    if min(*sizes, args.queries, args.samples, args.processes, args.prefix_iterations) <= 0 or args.cpu not in os.sched_getaffinity(0):
        parser.error("positive counts and an available CPU required")
    output = args.output.resolve()
    env = environment()
    if args.resume:
        report = json.loads((output / "results.json").read_text())
        assert not report.get("complete"), "completed campaigns cannot be resumed"
        for key in ("sizes", "queries", "samples", "processes", "cpu", "seed", "prefix_iterations", "suite"):
            assert report["configuration"][key] == getattr(args, key), f"resume configuration differs: {key}"
        previous_source = Path(report.get("active_source", str(output / "source")))
        verify_frozen(previous_source, report["dirty_source_sha256"])
        if "revisions" not in report:
            report["revisions"] = [{"revision": 0, "source": str(previous_source),
                                     "source_sha256": report["dirty_source_sha256"],
                                     "binaries_sha256": dict(report["binaries_sha256"])}]
            for run in report["runs"]:
                run["source_revision"] = 0
                run["binary_sha256"] = report["binaries_sha256"][run["suite"]]
        revision = len(report["revisions"])
        source = output / f"source-revision-{revision}"
        shutil.copytree(previous_source, source)
        # Only benchmark harness corrections enter a resumed source. Concurrent
        # production edits in the live checkout never enter an existing campaign.
        for p in source_files(ROOT):
            relative = p.relative_to(ROOT)
            if str(relative).startswith("benchmark/lookup-strategies/"):
                target = source / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(p.read_bytes())
        manifest = {str(p.relative_to(source)): sha(p) for p in source_files(source)}
        report["dirty_source_sha256"] = manifest
        report["active_source"] = str(source)
    else:
        output.mkdir(parents=True, exist_ok=False)
        source = output / "source"
        manifest = freeze(ROOT, source)
        revision = 0
        report = {"schema": 1, "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "machine": machine(args.cpu), "git_head": subprocess.check_output(
                  ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "dirty_source_sha256": manifest, "configuration": vars(args) | {"output": str(output)},
              "scope": "warm immutable snapshots; prefix is checked arena index-only; exact and ordered use file-backed snapshots",
                  "runs": []}
    (output / "provenance.json").write_text(json.dumps(report, indent=2) + "\n")
    suites = list(EXECUTABLES) if args.suite == "all" else [args.suite]
    binaries = {}
    for suite in suites:
        target = output / "target" / (f"revision-{revision}" if revision else "") / suite
        binary = target / "release" / EXECUTABLES[suite]
        command = ["cargo", "+1.94.0", "build", "--release", "--locked", "--manifest-path",
                   str(source / "benchmark/lookup-strategies" / suite / "Cargo.toml")]
        print(f"Building {suite}", flush=True)
        with (output / f"build-{suite}.log").open("w") as log:
            subprocess.run(command, cwd=source, env=env | {"CARGO_TARGET_DIR": str(target)},
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        binaries[suite] = binary
        report.setdefault("binaries_sha256", {})[suite] = sha(binary)
        if suite == "ordered":
            worker_target = output / "target" / (f"revision-{revision}" if revision else "") / "bbtree"
            worker_binary = worker_target / "release" / "ulmdb-bbtree-worker"
            worker_command = ["cargo", "+1.94.0", "build", "--release", "--locked", "--manifest-path",
                              str(source / "benchmark/lookup-strategies/ordered/bbtree/Cargo.toml")]
            with (output / "build-bbtree.log").open("w") as log:
                subprocess.run(worker_command, cwd=source, env=env | {"CARGO_TARGET_DIR": str(worker_target)},
                               stdout=log, stderr=subprocess.STDOUT, check=True)
            binaries["bbtree"] = worker_binary
            report["binaries_sha256"]["bbtree"] = sha(binaries["bbtree"])
    if args.resume:
        report["revisions"].append({"revision": revision, "source": str(source),
                                    "source_sha256": manifest, "binaries_sha256": dict(report["binaries_sha256"])})
    (output / "provenance.json").write_text(json.dumps(report, indent=2) + "\n")
    verify_frozen(source, manifest)
    # Fresh processes rotate suite/size order; variants rotate inside each process.
    jobs = []
    for size in sizes:
        for suite in suites:
            variants = [(b, k) for b in ("legacy", "resident") for k in ("fixed16", "hostname")] if suite == "exact" else [(None, None)]
            jobs += [(suite, size, backend, keys) for backend, keys in variants]
    for process in range(args.processes):
        order = jobs[process % len(jobs):] + jobs[:process % len(jobs)]
        for suite, size, backend, keys in order:
            if any((r["suite"], r["size"], r["process"], r.get("backend"), r.get("key_kind")) ==
                   (suite, size, process, backend, keys) for r in report["runs"]):
                continue
            variant = f"-{backend}-{keys}" if backend else ""
            stem = f"{suite}-{size}{variant}-p{process}"
            directory = output / "databases" / stem
            directory.parent.mkdir(parents=True, exist_ok=True)
            command = ["taskset", "-c", str(args.cpu), str(binaries[suite]),
                       "--queries", str(args.queries), "--seed", str(args.seed + process)]
            if suite == "prefix":
                command += ["--keys", str(size), "--rounds", str(args.samples), "--iterations", str(args.prefix_iterations)]
            elif suite == "exact":
                command += ["--rows", str(size), "--backend", backend, "--keys", keys,
                            "--samples", str(args.samples), "--directory", str(directory)]
            else:
                command += ["--keys", str(size), "--samples", str(args.samples), "--dir", str(directory),
                            "--bbtree-worker", str(binaries["bbtree"])]
            extension = ".csv" if suite == "prefix" else ".json"
            stdout = output / (stem + extension)
            stderr = output / (stem + ".stderr.log")
            if stdout.exists():
                attempt = len(report.setdefault("failed_attempts", []))
                archived_out = stdout.with_name(stdout.name + f".failed-attempt-{attempt}")
                archived_err = stderr.with_name(stderr.name + f".failed-attempt-{attempt}")
                shutil.move(stdout, archived_out)
                if stderr.exists():
                    shutil.move(stderr, archived_err)
                if directory.exists():
                    shutil.move(directory, directory.with_name(directory.name + f".failed-attempt-{attempt}"))
                report["failed_attempts"].append({"run": stem, "stdout": archived_out.name,
                                                   "stdout_sha256": sha(archived_out), "stderr": archived_err.name})
            print(f"Measuring {stem}", flush=True)
            with stdout.open("w") as out, stderr.open("w") as err:
                subprocess.run(command, cwd=source, env=env, stdout=out, stderr=err, check=True)
            if suite == "prefix":
                rows = list(csv.DictReader(io.StringIO(stdout.read_text())))
                assert rows and {r["kind"] for r in rows} >= {"build", "lookup"}
                data = {"rows": rows}
            elif suite == "exact":
                rows = [json.loads(line) for line in stdout.read_text().splitlines() if line.strip()]
                assert rows[-1]["type"] == "validation" and rows[-1]["passed"]
                data = {"rows": rows}
            else:
                data = json.loads(stdout.read_text())
            validate(suite, data, size, args.samples, args.seed + process, backend, keys, args.queries)
            report["runs"].append({"suite": suite, "size": size, "process": process,
                                   "backend": backend, "key_kind": keys,
                                   "source_revision": revision, "binary_sha256": sha(binaries[suite]),
                                   "seed": args.seed + process, "raw_file": stdout.name,
                                   "raw_sha256": sha(stdout), "report": data})
            verify_frozen(source, manifest)
            (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    report["complete"] = True
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Evidence: {output / 'results.json'}", flush=True)


if __name__ == "__main__":
    main()
