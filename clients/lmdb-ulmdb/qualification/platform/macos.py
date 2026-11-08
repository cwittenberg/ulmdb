#!/usr/bin/env python3
"""Build and exercise the existing native macOS package on an actual Intel Mac.

This driver deliberately refuses a cross host. Its receipt binds the source,
Rust test executables, packaged provider and C tools; a successful cross build
or a libtest target containing zero tests is not a runtime result.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
HERE = ROOT / "clients/lmdb-ulmdb"
TOOLCHAIN = "1.94.0"
HOST = "x86_64-apple-darwin"
XCODE = "/Applications/Xcode_16.4.app/Contents/Developer"
SDK = "15.5"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            result.update(block)
    return result.hexdigest()


def source_inventory():
    # Reuse the main gate's actual source selection; include the Apple link
    # policy and installed guide which the native packager additionally reads.
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("qualification", HERE / "qualify.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = module.source_fingerprint()
    for relative in ("exports.darwin", "OPERATOR.md", "README.md"):
        result[str((HERE / relative).relative_to(ROOT))] = digest(HERE / relative)
    return result


def require_executed_tests(text, names):
    lines = [line for line in text.splitlines() if line.startswith("test result:")]
    summaries = [re.match(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;", line)
                 for line in lines]
    require(summaries and all(summary is not None and summary[1] == "ok"
                             and int(summary[2]) > 0 and summary[3] == summary[4] == "0"
                             for summary in summaries), "missing, ignored, failed or zero-test runtime target")
    # A process fixture can emit successful child libtest summaries. The last
    # summary belongs to the selected parent; its exact test must also finish.
    require(int(summaries[-1][2]) == len(names), "selected parent test count mismatch")
    for name in names:
        require(re.search(r"^test " + re.escape(name) + r" \.\.\. ok$", text, re.M),
                "required test did not execute successfully: " + name)


def group_members(leader):
    # WNOWAIT keeps the group leader PID reserved until cleanup finishes. Do
    # not use Popen.poll()/wait() before signaling its group.
    # P_PID must still denote our child. Unlike platform-specific zombie
    # getpgid/getsid behavior, waitid authenticates both a live child and an
    # unreaped terminal child without releasing its PID.
    os.waitid(os.P_PID, leader, os.WEXITED | os.WNOWAIT | os.WNOHANG)
    output = subprocess.check_output(["ps", "-axo", "pid=,pgid=,stat="], text=True, timeout=10)
    return [int(fields[0]) for line in output.splitlines()
            if len(fields := line.split()) == 3 and int(fields[1]) == leader
            and not fields[2].startswith(("Z", "X"))]


class Run:
    def __init__(self, output, timeout):
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.timeout = timeout
        self.env = os.environ.copy()
        for name in list(self.env):
            if name.startswith(("ULMDB_", "DYLD_", "LD_")):
                self.env.pop(name)
        self.env.pop("RUST_TEST_THREADS", None)
        self.env.update(RUSTUP_TOOLCHAIN=TOOLCHAIN, CARGO_TARGET_DIR=str(self.output / "cargo"),
                        PYTHONDONTWRITEBYTECODE="1", ULMDB_LMDB_ABI="1.0")
        self.receipt = {"schema": 1, "profile": "native-macos-host-v1", "complete": False,
                        "passed": False, "host_admitted": False, "commands": [], "artifacts": {},
                        "scope": "actual Intel macOS kernel, C/package/operator runtime; no power-loss or performance claim",
                        "host": {"system": sys.platform, "machine": platform.machine(),
                                 "python": sys.version, "image": os.environ.get("ImageVersion")}}
        self.save()

    def save(self):
        (self.output / "receipt.json").write_text(json.dumps(self.receipt, indent=2) + "\n")

    def bind(self, path):
        path = Path(path).resolve()
        actual = digest(path)
        require(self.receipt["artifacts"].get(str(path), actual) == actual,
                "bound executable/provider changed: " + str(path))
        self.receipt["artifacts"][str(path)] = actual
        self.save()

    def command(self, label, arguments, *, env=None, timeout=None):
        path = self.output / (f"{len(self.receipt['commands']) + 1:02}-" + label + ".log")
        row = {"label": label, "arguments": [str(x) for x in arguments], "log": str(path),
               "terminal": False}
        self.receipt["commands"].append(row)
        self.save()
        print("RUN " + label, flush=True)
        process = None
        started = time.monotonic()
        try:
            with path.open("wb") as log:
                pending = []
                def defer(signum, _frame):
                    pending.append(signum)
                previous = {s: signal.signal(s, defer) for s in (signal.SIGINT, signal.SIGTERM)}
                try:
                    process = subprocess.Popen(row["arguments"], cwd=ROOT, env=env or self.env,
                                               stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                finally:
                    for signum, handler in previous.items():
                        signal.signal(signum, handler)
                row["pid"] = process.pid
                if pending:
                    raise KeyboardInterrupt("interrupted during stage creation")
                deadline = started + (self.timeout if timeout is None else timeout)
                while True:
                    status = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
                    if status is not None:
                        row["exit_code"] = status.si_status if status.si_code == os.CLD_EXITED else -status.si_status
                        break
                    require(time.monotonic() < deadline, "stage deadline: " + label)
                    time.sleep(0.05)
        finally:
            previous = {s: signal.signal(s, signal.SIG_IGN) for s in (signal.SIGINT, signal.SIGTERM)}
            try:
                if process is not None:
                    try:
                        try:
                            row["remaining_before_cleanup"] = group_members(process.pid)
                        finally:
                            os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
                            try:
                                os.killpg(process.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        deadline = time.monotonic() + 5
                        while group_members(process.pid):
                            require(time.monotonic() < deadline, "owned group did not stop: " + label)
                            time.sleep(0.05)
                        row["remaining_after_cleanup"] = []
                    finally:
                        process.wait(timeout=10)
                        row.setdefault("exit_code", process.returncode)
                        row["terminal"] = True
            finally:
                for signum, handler in previous.items():
                    signal.signal(signum, handler)
                row["seconds"] = round(time.monotonic() - started, 3)
                if path.exists():
                    row["log_sha256"] = digest(path)
                self.save()
        require(row.get("exit_code") == 0, "stage failed: " + label + "; see " + str(path))
        require(not row["remaining_before_cleanup"], "successful stage leaked descendants: " + label)
        return path.read_text(errors="replace")


def fetch_modern(run):
    pin = json.loads((HERE / "modern-source.json").read_text())
    target = run.output / "sources/lmdb-1.0.2"
    target.mkdir(parents=True)
    for entry in pin["files"]:
        name = entry["path"]
        require(Path(name).name == name, "nonlocal source pin")
        url = f"https://raw.githubusercontent.com/LMDB/lmdb/{pin['commit']}/libraries/liblmdb/{name}"
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(entry["bytes"] + 1)
        require(len(data) == entry["bytes"] and hashlib.sha256(data).hexdigest() == entry["sha256"],
                "pinned modern source mismatch: " + name)
        with (target / name).open("xb") as stream:
            stream.write(data)
        run.bind(target / name)
    return target


def test_executables(text, targets):
    found = {}
    for line in text.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (event.get("reason") == "compiler-artifact" and event.get("executable")
                and event.get("profile", {}).get("test")):
            name = event["target"]["name"]
            require(name not in found, "ambiguous test executable: " + name)
            found[name] = Path(event["executable"])
    require(set(found) == set(targets), "missing or unexpected Rust test executables")
    return found


def run_tests(run, executable, label, names):
    run.bind(executable)
    listing = run.command(label + "-list", [executable, "--list", "--format", "terse"])
    for name in names:
        require(name + ": test" in listing.splitlines(), "test absent on this host: " + name)
    for index, name in enumerate(names):
        text = run.command(label + "-" + str(index + 1),
                           [executable, "--exact", name, "--test-threads=2", "--nocapture"])
        require_executed_tests(text, [name])


def qualify(run):
    require(sys.platform == "darwin" and platform.machine() == "x86_64", "requires a native Intel macOS host")
    require(sys.version_info >= (3, 13) and hasattr(os, "waitid"), "macOS WNOWAIT cleanup requires Python >=3.13")
    require(run.env.get("DEVELOPER_DIR") == XCODE, "select the pinned Xcode16.4 DEVELOPER_DIR")
    run.receipt["host_admitted"] = True
    run.receipt["source_before"] = source_inventory()
    run.save()
    require(run.command("sdk-version", ["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-version"]).strip() == SDK,
            "unexpected Apple SDK version")
    sdk = run.command("sdk-path", ["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path"]).strip()
    run.env["SDKROOT"] = sdk
    compiler = run.command("compiler-path", ["/usr/bin/xcrun", "--find", "clang"]).strip()
    run.env["CC"] = compiler
    require("Xcode 16.4" in run.command("xcode-version", ["/usr/bin/xcodebuild", "-version"]), "wrong Xcode")
    run.command("compiler-version", [compiler, "--version"])
    version = run.command("rust-version", ["rustc", "+" + TOOLCHAIN, "--version", "--verbose"])
    require("release: " + TOOLCHAIN in version and "host: " + HOST in version, "wrong native Rust toolchain")
    cargo = ["cargo", "+" + TOOLCHAIN, "test", "--locked", "--no-run", "--message-format=json"]
    text = run.command("build-platform-tests", cargo + ["-p", "ulmdb-platform", "--lib"])
    platform_tests = test_executables(text, ["ulmdb_platform"])["ulmdb_platform"]
    run_tests(run, platform_tests, "platform", [
        "tests::independent_range_owners_survive_other_owners_and_unrelated_descriptor_close",
        "tests::relative_file_open_is_rooted_and_rejects_parent_traversal",
        "tests::private_artifacts_are_exclusive_nofollow_and_really_sealed",
        "tests::namespace_replace_retains_old_open_object_and_fences_held_directory"])
    targets = {"fixed_portable_smoke": ["portable_fixed_plain_mapped_old_borrow_and_reopen"],
               "resident_capture_identity": ["negotiated_capture_binds_actual_platform_identity_and_retains_old_bytes"],
               "resident_export": ["pinned_complete_export_survives_source_writes_collection_owner_drop_and_removal",
                                   "transformed_export_retains_callbacks_and_cleans_failed_owned_staging"],
               "resident_checkpoint_faults": ["checkpoint_errors_and_death_preserve_exact_business_head_and_writer_progress"]}
    arguments = cargo + ["-p", "ulmdb", "--features", "fault-injection"]
    for target in targets:
        arguments += ["--test", target]
    tests = test_executables(run.command("build-native-tests", arguments), targets)
    for target, names in targets.items():
        run_tests(run, tests[target], target, names)
    run.command("build-provider", ["cargo", "+" + TOOLCHAIN, "build", "--locked", "--release", "-p", "ulmdb-c"])
    archive = run.output / "cargo/release/libulmdb_c.a"
    run.bind(archive)
    modern = fetch_modern(run)
    packager = run.output / "ulmdb-adapter-build"
    run.command("build-existing-packager", ["make", "-C", HERE, "builder", "CC=" + compiler,
                "TOOLS=" + str(run.output / "builder-tools"), "PACKAGER=" + str(packager)])
    run.bind(packager)
    package = run.output / "package"
    run.command("package-native-darwin", [packager, "--no-cargo", "--profile", "release", "--output", package,
                "--lmdb-source", modern, "--legacy-source", run.output / "sources/lmdb-0.9.31", "--fetch-legacy"])
    provider = package / "lib/liblmdb.0.dylib"
    binaries = [package / "bin" / name for name in ("ulmdb-tool", "ulmdb-migrate", "lmdb-export",
                "lmdb-export-0.9", "convert-lmdb-to-ulmdb")]
    for path in [provider, *binaries]:
        run.bind(path)
        text = run.command("macho-" + path.name, ["/usr/bin/file", path])
        require("Mach-O 64-bit" in text and "x86_64" in text, "not a native Intel Mach-O: " + str(path))
    test = run.output / "reserve-ordering"
    run.command("build-public-c", [compiler, "-std=c11", "-O2", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror",
                "-I" + str(package / "include"), ROOT / "clients/c/tests/reserve_ordering.c",
                "-L" + str(package / "lib"), "-llmdb", "-Xlinker", "-rpath", "-Xlinker", package / "lib", "-o", test])
    run.bind(test)
    require("@rpath/liblmdb.0.dylib" in run.command("public-c-imports", ["/usr/bin/otool", "-L", test]),
            "public C executable does not import the packaged provider")
    observed = dict(run.env, DYLD_PRINT_LIBRARIES="1")
    text = run.command("public-c-lifecycle", [test, run.output / "public-c-db"], env=observed)
    require(str(provider) in text and "reserve-ordering-ok:" in text, "missing actual provider/lifecycle proof")
    text = run.command("operator-provider", [binaries[0], "--help"], env=observed)
    require(str(provider) in text, "operator did not load the bound provider")
    run.command("launcher-public-c", [binaries[-1], "--abi", "1.0.2", test, run.output / "launcher-db"])
    text = run.command("operator-pipes", [sys.executable, HERE / "test_operator.py", "--package", package])
    require("operator pipe/binary/flags/empty/duplicates/atomic/copy/reopen: PASS" in text, "operator journey incomplete")
    run.receipt["package_files"] = {str(p.relative_to(package)): digest(p)
                                    for p in sorted(package.rglob("*")) if p.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--stage-timeout", type=int, default=900)
    args = parser.parse_args()
    require(0 < args.stage_timeout <= 1800, "invalid stage deadline")
    def interrupted(signum, _frame):
        raise KeyboardInterrupt("macOS job interrupted by signal " + str(signum))
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, interrupted)
    run = Run(args.output, args.stage_timeout)
    try:
        qualify(run)
        require(run.receipt["source_before"] == source_inventory(), "source drift")
        require(all(digest(p) == h for p, h in run.receipt["artifacts"].items()), "provider/executable drift")
        run.receipt["passed"] = True
    except BaseException as error:
        run.receipt["error"] = repr(error)
        raise
    finally:
        if "source_before" in run.receipt:
            try:
                run.receipt["source_after"] = source_inventory()
                run.receipt["source_stable"] = run.receipt["source_before"] == run.receipt["source_after"]
            except BaseException as error:
                run.receipt.update(source_stable=False, source_after_error=repr(error))
            if not run.receipt["source_stable"]:
                run.receipt["passed"] = False
        run.receipt["complete"] = True
        run.save()


if __name__ == "__main__":
    main()
