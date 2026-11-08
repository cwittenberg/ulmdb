#!/usr/bin/env python3
"""Opt-in flat CPU sampling without perf_event privileges; profiling results only."""
import argparse
import collections
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
LIBRARY = ROOT / "benchmark/.cache/profile/libcpu_sampler.so"


def build():
    LIBRARY.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        os.environ.get("CC", "cc"), "-std=c11", "-O2", "-fPIC", "-shared",
        "-Wall", "-Wextra", "-Werror", str(HERE / "cpu_sampler.c"),
        "-o", str(LIBRARY), "-ldl",
    ], check=True)
    return LIBRARY


def digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def symbolize(path, addresses):
    if not Path(path).is_file():
        return {}
    resolved = {}
    addresses = sorted(addresses)
    # Bounded subprocess batches, with addresses on stdin rather than argv.
    for start in range(0, len(addresses), 5000):
        chunk = addresses[start:start + 5000]
        result = subprocess.run(
            ["addr2line", "-a", "-f", "-C", "-i", "-e", path],
            input="".join(f"0x{address:x}\n" for address in chunk),
            text=True, capture_output=True, check=True,
        )
        current = None
        frames = []
        for line in result.stdout.splitlines():
            if re.fullmatch(r"0x[0-9a-fA-F]+", line):
                if current is not None:
                    resolved[current] = tuple(zip(frames[::2], frames[1::2]))
                current = int(line, 16)
                frames = []
            else:
                frames.append(line)
        if current is not None:
            resolved[current] = tuple(zip(frames[::2], frames[1::2]))
    return resolved


def report(output, report_path=None, executable=None, top=40,
           start_ms=0.0, end_ms=None):
    output = Path(output)
    metadata = json.loads(Path(str(output) + ".meta.json").read_text())
    modules = metadata["modules"]
    if executable:
        for module in modules:
            if module["main"]:
                module["path"] = str(Path(executable).resolve())
    ranges = [(int(begin, 16), int(end, 16), index, int(module["load_bias"], 16))
              for index, module in enumerate(modules)
              for begin, end in module["exec_ranges"]]
    counts = collections.Counter()
    selected = 0
    first_ns = last_ns = None
    with output.open(newline="") as source:
        for row in csv.DictReader(source):
            elapsed_ns = int(row["elapsed_ns"])
            if elapsed_ns < start_ms * 1_000_000:
                continue
            if end_ms is not None and elapsed_ns >= end_ms * 1_000_000:
                continue
            pc = int(row["pc"], 16)
            index, address = -1, pc
            for begin, end, candidate, bias in ranges:
                if begin <= pc < end:
                    index, address = candidate, pc - bias
                    break
            counts[index, address] += 1
            selected += 1
            first_ns = elapsed_ns if first_ns is None else min(first_ns, elapsed_ns)
            last_ns = elapsed_ns if last_ns is None else max(last_ns, elapsed_ns)
    resolved = {}
    for index, module in enumerate(modules):
        addresses = {address for candidate, address in counts if candidate == index}
        if addresses:
            resolved[index] = symbolize(module["path"], addresses)
    flat = collections.Counter()
    locations = collections.Counter()
    module_counts = collections.Counter()
    inline = collections.Counter()
    for (index, address), count in counts.items():
        name = modules[index]["path"] if index >= 0 else "<unmapped>"
        module_counts[name] += count
        frames = resolved.get(index, {}).get(address, ())
        if not frames:
            frames = ((f"{Path(name).name}+0x{address:x}", "??:0"),)
        function, location = frames[0]
        flat[function] += count
        locations[function, location] += count
        # DWARF inline expansion only: these are not runtime caller stacks.
        for function in {function for function, _ in frames}:
            inline[function] += count
    lines = [
        "Profiling-only flat CPU sample attribution",
        f"Source: {output.resolve()}",
        f"Timer interval: {metadata['interval_us']} us requested; delivered samples: "
        f"{metadata['samples']}; selected: {selected}; dropped at capacity: "
        f"{metadata['dropped_at_capacity']}",
        f"Selected sample span: {None if first_ns is None else first_ns / 1e9} .. "
        f"{None if last_ns is None else last_ns / 1e9} seconds after sampler start",
        "Percentages use selected samples, including unresolved/shared-library samples.",
        "ITIMER_PROF ticks can coalesce and are subject to kernel timer granularity.",
        "RIP identifies interrupted user code; no kernel stacks, runtime caller stacks, "
        "wall-time breakdown or workload phase inference.",
        "Sampling changes execution; timings from this run are NOT accepted benchmark results.",
    ]
    for title, counter in [
        ("Modules", module_counts), ("Flat functions (innermost DWARF inline frame)", flat),
        ("Instruction locations", locations),
        ("Inline-inclusive functions (inline expansion only; percentages may overlap)", inline),
    ]:
        lines.extend(["", title])
        for key, count in counter.most_common(top):
            label = " @ ".join(key) if isinstance(key, tuple) else key
            percentage = count * 100 / selected if selected else 0
            lines.append(f"{count:8d} {percentage:7.2f}%  {label}")
    report_path = Path(report_path or str(output) + ".report.txt")
    report_path.write_text("\n".join(lines) + "\n")
    return report_path


def run(args):
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        raise ValueError("run requires a command after --")
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists() or Path(str(output) + ".meta.json").exists():
        raise ValueError("output or metadata exists; choose a fresh output path")
    library = build()
    env = os.environ.copy()
    env.update(ULMDB_PC_PROFILE=str(output), ULMDB_PC_INTERVAL_US=str(args.interval_us),
               ULMDB_PC_MAX_SAMPLES=str(args.max_samples))
    env["LD_PRELOAD"] = str(library) + (
        ":" + env["LD_PRELOAD"] if env.get("LD_PRELOAD") else "")
    invocation = ["taskset", "-c", args.cpu, *command] if args.cpu else command
    # Set LD_PRELOAD only on the actual target; profiling taskset itself would
    # install a second sampler before exec and could interfere with the timer.
    if args.cpu:
        invocation = ["taskset", "-c", args.cpu, "env",
                      "LD_PRELOAD=" + env.pop("LD_PRELOAD"), *command]
        for key in ("ULMDB_PC_PROFILE", "ULMDB_PC_INTERVAL_US", "ULMDB_PC_MAX_SAMPLES"):
            invocation.insert(4, f"{key}={env.pop(key)}")
    print("Profiling only: benchmark timings from this process are not qualifying results.",
          file=sys.stderr)
    result = subprocess.run(invocation, env=env)
    metadata_path = Path(str(output) + ".meta.json")
    if not output.exists() or not metadata_path.exists():
        raise RuntimeError("sampler output missing (unsupported target, signal conflict, "
                           "exec, abrupt exit or initialization failure)")
    metadata = json.loads(metadata_path.read_text())
    target = shutil.which(command[0]) or command[0]
    metadata["invocation"] = {
        "command": command, "cpu": args.cpu, "returncode": result.returncode,
        "executable_sha256": digest(target) if Path(target).is_file() else None,
        "sampler_sha256": digest(library), "qualifying_benchmark": False,
    }
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    generated = report(output, args.report, top=args.top)
    print(f"Samples: {output}\nMetadata: {metadata_path}\nHotspots: {generated}",
          file=sys.stderr)
    return result.returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    sub.add_parser("build")
    runner = sub.add_parser("run")
    runner.add_argument("--output", required=True)
    runner.add_argument("--report")
    runner.add_argument("--interval-us", type=int, default=1000)
    runner.add_argument("--max-samples", type=int, default=1000000)
    runner.add_argument("--cpu", help="taskset CPU list, for example 4 or 4-5")
    runner.add_argument("--top", type=int, default=40)
    runner.add_argument("command", nargs=argparse.REMAINDER)
    reporter = sub.add_parser("report")
    reporter.add_argument("output")
    reporter.add_argument("--report")
    reporter.add_argument("--executable", help="same exact main ELF at a new path")
    reporter.add_argument("--top", type=int, default=40)
    reporter.add_argument("--start-ms", type=float, default=0)
    reporter.add_argument("--end-ms", type=float)
    args = parser.parse_args()
    if args.operation == "build":
        print(build())
        return 0
    if args.operation == "run":
        return run(args)
    print(report(args.output, args.report, args.executable, args.top,
                 args.start_ms, args.end_ms))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"profile: {error}", file=sys.stderr)
        sys.exit(1)
