#!/usr/bin/env python3
"""Prove features-v2 rejects real missing-export and wrong-behavior libraries.

Build the ordinary package and parity runner first. Each control relinks the
existing Rust static archive into an isolated package; qualified inputs and
checked-in reports are never modified. Reports, traces and a control receipt
remain under --output, which must be a new directory.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[3]
SYMBOL = "mdb_txn_flags"
EXPORT_SCENARIO = "LMDB 1.0 transaction flags export"
BEHAVIOR_SCENARIO = "LMDB 1.0 transaction flags"
# This fixture also calls mdb_txn_flags and asserts MDB_RDONLY on its private
# children. Both deficient libraries must fail it for the same injected defect.
CHILD_FLAGS_SCENARIO = "LMDB 1.0 read-only child transactions"
# These suites query public flags on dirty pending WRITE owners. Hiding the
# symbol must fail them; a bogus zero flag value still matches those writers.
PREPARED_FLAG_SCENARIOS = {
    "LMDB 1.0 transaction preparation",
    "LMDB 1.0 prepared metadata helpers",
}


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, timeout=120, **kwargs)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def exported_symbols(library: Path) -> set[str]:
    output = run(["nm", "-D", "--defined-only", str(library)], capture_output=True).stdout
    return {
        line.split()[-1] for line in output.splitlines()
        if line.split() and re.fullmatch(r"(?:mdb|ulmdb)_[A-Za-z0-9_]+", line.split()[-1])
    }


def package_without_library(source: Path, destination: Path) -> None:
    # The package's 100+ MiB static archive is unnecessary to run dynamic apps.
    # Do not hardlink anything: no control write can change qualified inputs.
    shutil.copytree(source, destination, symlinks=True,
                    ignore=lambda directory, _names: ["lib"] if Path(directory) == source else [])
    library = destination / "lib"
    library.mkdir()
    for alias in ("liblmdb.so", "liblmdb.so.1", "libulmdb_c.so"):
        (library / alias).symlink_to("liblmdb.so.0")


def build_control(kind: str, directory: Path, args: argparse.Namespace,
                  exports: set[str], compiler: list[str]) -> Path:
    directory.mkdir()
    package = directory / "package"
    package_without_library(args.package, package)
    required_exports = exports - {SYMBOL} if kind == "missing-export" else exports
    version_script = directory / "exports.map"
    version_script.write_text("{\n  global:\n" + "".join(
        f"    {symbol};\n" for symbol in sorted(required_exports)) + "  local: *;\n};\n")
    library = package / "lib/liblmdb.so.0"
    command = compiler + ["-shared", "-Wl,-soname,liblmdb.so.0",
                          f"-Wl,--version-script={version_script}"]
    if kind == "wrong-trace":
        source = directory / "wrong_flags.c"
        source.write_text(
            "#include <errno.h>\n"
            "typedef struct MDB_txn MDB_txn;\n"
            "int mdb_txn_flags(MDB_txn *txn, unsigned int *flags) {\n"
            "    if (!txn || !flags) return EINVAL;\n"
            "    *flags = 0; /* Deliberately omit MDB_RDONLY on readers. */\n"
            "    return 0;\n"
            "}\n")
        obj = directory / "wrong_flags.o"
        run(compiler + ["-std=c11", "-O2", "-fPIC", "-Wall", "-Wextra", "-Werror",
                        "-c", str(source), "-o", str(obj)])
        # GNU ld keeps the first definition. This changes the real public
        # function used by the unchanged fixture, while retaining other APIs.
        command += ["-Wl,--allow-multiple-definition", str(obj)]
    command += ["-Wl,--whole-archive", str(args.archive), "-Wl,--no-whole-archive",
                "-ldl", "-lpthread", "-lm", "-lrt", "-lutil", "-o", str(library)]
    run(command)
    observed = exported_symbols(library)
    require(observed == required_exports, f"{kind}: control exports differ unexpectedly")
    return package


def verify_failure(kind: str, report: Path, returncode: int) -> dict[str, object]:
    receipt_file = report / "receipt.json"
    require(returncode != 0, f"{kind}: deficient library was accepted")
    require(receipt_file.is_file(), f"{kind}: failure receipt was not retained")
    receipt = json.loads(receipt_file.read_text())
    require(receipt["profile"] == "lmdb-features-v2", f"{kind}: wrong profile")
    require(receipt["complete"] and not receipt["passed"],
            f"{kind}: must complete the suite with a recorded failure")
    common = receipt["common_symbols"]
    require(common["declared"] == common["exported"] == 56,
            f"{kind}: unrelated common exports failed")
    expected = {EXPORT_SCENARIO, BEHAVIOR_SCENARIO, CHILD_FLAGS_SCENARIO} | PREPARED_FLAG_SCENARIOS
    results = {item["scenario"]: item for item in receipt["results"]
               if item["scenario"] in expected}
    require(set(results) == expected,
            f"{kind}: expected flags export and dependent scenarios missing")
    export = results[EXPORT_SCENARIO]
    behavior = results[BEHAVIOR_SCENARIO]
    require(export["passed"] == (kind == "wrong-trace"),
            f"{kind}: export check did not identify the intended defect")
    require(not behavior["passed"] and behavior["reference_exit"] == 0
            and behavior["adapter_exit"] != 0,
            f"{kind}: behavior scenario did not reject the faulty implementation")
    require(behavior["reference_trace_sha256"] != behavior["adapter_trace_sha256"],
            f"{kind}: behavior traces did not diverge")
    children = results[CHILD_FLAGS_SCENARIO]
    require(not children["passed"] and children["reference_exit"] == 0
            and children["adapter_exit"] != 0,
            f"{kind}: child flags did not reject the faulty implementation")
    require(children["reference_trace_sha256"] != children["adapter_trace_sha256"],
            f"{kind}: child flags traces did not diverge")
    for scenario in PREPARED_FLAG_SCENARIOS:
        pending = results[scenario]
        require(pending["reference_exit"] == 0,
                f"{kind}: pinned prepared reference failed: {scenario}")
        if kind == "missing-export":
            require(not pending["passed"] and pending["adapter_exit"] != 0
                    and pending["reference_trace_sha256"] != pending["adapter_trace_sha256"],
                    f"{kind}: prepared flags dependency did not reject missing export: {scenario}")
        else:
            require(pending["passed"] and pending["adapter_exit"] == 0
                    and pending["reference_trace_sha256"] == pending["adapter_trace_sha256"],
                    f"{kind}: write-only prepared flags should still be zero: {scenario}")
    unrelated = [item["scenario"] for item in receipt["results"]
                 if not item["passed"] and item["scenario"] not in results]
    require(not unrelated, f"{kind}: unrelated failures: {unrelated}")
    failures = sorted((report / "failures").glob("*.txt"))
    require(len(failures) >= 2, f"{kind}: reference/adapter failure traces not retained")
    return {"kind": kind, "runner_exit": returncode,
            "receipt": str(receipt_file), "receipt_sha256": digest(receipt_file),
            "failed_scenarios": [item["scenario"] for item in receipt["results"]
                                 if not item["passed"]],
            "failure_traces": [{"path": str(path), "sha256": digest(path)} for path in failures]}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, default=ROOT / "target/lmdb-ulmdb")
    parser.add_argument("--archive", type=Path, default=ROOT / "target/debug/libulmdb_c.a")
    parser.add_argument("--runner", type=Path, default=ROOT / "target/lmdb-ulmdb-parity")
    parser.add_argument("--output", type=Path, default=ROOT / "target/lmdb-negative-controls")
    for name in ("lmdb-source", "legacy-source", "current09-source"):
        parser.add_argument(f"--{name}", type=Path)
    args = parser.parse_args()
    for name in ("package", "archive", "runner", "output"):
        setattr(args, name, getattr(args, name).resolve())
    library = args.package / "lib/liblmdb.so.0"
    inputs = [library, args.archive, args.runner,
              ROOT / "clients/lmdb-ulmdb/conformance/receipt.json",
              ROOT / "clients/lmdb-ulmdb/conformance/REPORT.md"]
    before = {str(path): digest(path) for path in inputs}
    exports = exported_symbols(library)
    require(SYMBOL in exports, "qualified package must export mdb_txn_flags before controls")
    args.output.mkdir(parents=True, exist_ok=False)
    compiler = shlex.split(os.environ.get("CC", "cc"))
    environment = os.environ.copy()
    for name in ("LD_PRELOAD", "LD_LIBRARY_PATH", "ULMDB_LIBRARY", "ULMDB_MIGRATOR"):
        environment.pop(name, None)
    controls = []
    for kind in ("missing-export", "wrong-trace"):
        directory = args.output / kind
        package = build_control(kind, directory, args, exports, compiler)
        report = directory / "report"
        command = [str(args.runner), "--package", str(package), "--profile", "features-v2",
                   "--report-dir", str(report), "--operations", "64", "--keep-traces"]
        for name in ("lmdb_source", "legacy_source", "current09_source"):
            source = getattr(args, name)
            if source is not None:
                command += ["--" + name.replace("_", "-"), str(source.resolve())]
        with (directory / "runner.log").open("w") as log:
            result = subprocess.run(command, text=True, stdout=log, stderr=subprocess.STDOUT,
                                    timeout=600, env=environment)
        control = verify_failure(kind, report, result.returncode)
        control["library_sha256"] = digest(package / "lib/liblmdb.so.0")
        controls.append(control)
        print(f"PASS: {kind} rejected; failure receipt and traces: {report}", flush=True)
    require(before == {str(path): digest(path) for path in inputs},
            "qualified inputs or baseline reports changed during controls")
    summary = {"schema": 1, "passed": True, "profile": "lmdb-features-v2",
               "input_sha256": before, "controls": controls}
    (args.output / "controls.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"PASS: qualified inputs unchanged; control receipt: {args.output / 'controls.json'}")


if __name__ == "__main__":
    main()
