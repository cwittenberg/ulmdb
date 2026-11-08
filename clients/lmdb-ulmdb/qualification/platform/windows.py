#!/usr/bin/env python3
"""Native Windows GNU/MSVCRT host checks. Cross-linking is not a runtime pass.

Reuses the repository's builders, public C fixtures and exact portable Rust
tests. Every child starts suspended inside a kill-on-close Job before executing.
No receipt here changes the canonical Linux qualification or parity registry.
"""
from __future__ import annotations

import argparse
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time
import types
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
HERE = ROOT / "clients/lmdb-ulmdb"
TARGET = "x86_64-pc-windows-gnu"
RUST = "1.94.0"
TOOLCHAIN = {
    "release": "https://github.com/skeeto/w64devkit/releases/tag/v2.11.0",
    "url": "https://github.com/skeeto/w64devkit/releases/download/v2.11.0/w64devkit-x64-2.11.0.7z.exe",
    "bytes": 68801240,
    "sha256": "54c2bb9962e8a30c8270e34c5efc3d5fee579f53087fa7d435b95fbba2af7b2b",
}
SUSPENDED_GROUP = 0x4 | 0x200


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def checked_file(path, expected, size=None):
    if (size is not None and path.stat().st_size != size) or sha(path) != expected:
        raise RuntimeError("source/artifact mismatch: " + str(path))


def source_module(path, name):
    # Execute these owned source bytes, never an unrecorded __pycache__ entry.
    module = types.ModuleType(name)
    module.__file__ = str(path)
    exec(compile(path.read_bytes(), str(path), "exec"), module.__dict__)
    return module


def sources():
    return source_module(HERE / "qualify.py", "platform_source_inventory").source_fingerprint()


def require_windows():
    if os.name != "nt" or platform.machine().upper() not in ("AMD64", "X86_64"):
        raise RuntimeError("requires a genuine native Windows x86_64 host; no cross/runtime substitution")
    if sys.version_info < (3, 13):
        raise RuntimeError("Python 3.13 or newer required")


class BasicLimits(C.Structure):
    _fields_ = [("process_time", C.c_int64), ("job_time", C.c_int64),
                ("flags", C.c_uint32), ("min_ws", C.c_size_t), ("max_ws", C.c_size_t),
                ("active", C.c_uint32), ("affinity", C.c_size_t),
                ("priority", C.c_uint32), ("scheduling", C.c_uint32)]


class ExtendedLimits(C.Structure):
    _fields_ = [("basic", BasicLimits), ("io", C.c_uint64 * 6),
                ("process_memory", C.c_size_t), ("job_memory", C.c_size_t),
                ("peak_process", C.c_size_t), ("peak_job", C.c_size_t)]


class Job:
    """Owned HANDLEs, not a reused numeric PID, delimit child tree authority."""
    def __init__(self):
        self.k = C.WinDLL("kernel32", use_last_error=True)
        self.n = C.WinDLL("ntdll")
        self.k.CreateJobObjectW.argtypes = [C.c_void_p, C.c_wchar_p]
        self.k.CreateJobObjectW.restype = C.c_void_p
        self.k.SetInformationJobObject.argtypes = [C.c_void_p, C.c_int, C.c_void_p, C.c_uint32]
        self.k.SetInformationJobObject.restype = C.c_int
        self.k.AssignProcessToJobObject.argtypes = [C.c_void_p, C.c_void_p]
        self.k.AssignProcessToJobObject.restype = C.c_int
        self.k.CloseHandle.argtypes = [C.c_void_p]
        self.k.CloseHandle.restype = C.c_int
        self.n.NtResumeProcess.argtypes = [C.c_void_p]
        self.n.NtResumeProcess.restype = C.c_int32
        self.handle = self.k.CreateJobObjectW(None, None)
        if not self.handle:
            raise C.WinError(C.get_last_error())
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self.k.SetInformationJobObject(self.handle, 9, C.byref(limits), C.sizeof(limits)):
            error = C.WinError(C.get_last_error())
            self.close()
            raise error

    def attach_and_resume(self, process):
        # Popen owns this genuine HANDLE until wait/close. No child instruction
        # ran before assignment, so descendants cannot escape during admission.
        if not self.k.AssignProcessToJobObject(self.handle, int(process._handle)):
            raise C.WinError(C.get_last_error())
        status = self.n.NtResumeProcess(int(process._handle))
        if status < 0:
            raise RuntimeError("NtResumeProcess failed: " + hex(status & 0xffffffff))

    def close(self):
        if self.handle:
            handle, self.handle = self.handle, None
            if not self.k.CloseHandle(handle):
                raise C.WinError(C.get_last_error())


def rust_pass(output, name):
    # A cfg-disabled/renamed test or a successful zero-test executable is a fail.
    text = output.decode("utf-8", "replace")
    if not re.search(r"^test " + re.escape(name) + r" \.\.\. ok\r?$", text, re.M):
        raise RuntimeError("requested native test did not execute: " + name)
    summaries = re.findall(r"^test result: [^\r\n]+", text, re.M)
    counts = []
    for summary in summaries:
        match = re.fullmatch(r"test result: ok\. ([1-9][0-9]*) passed; 0 failed; 0 ignored;.*", summary)
        if not match:
            raise RuntimeError("native child/parent zero-test, failed or ignored result: " + summary)
        counts.append(int(match[1]))
    if not counts or counts[-1] != 1:
        raise RuntimeError("native final parent count/result mismatch: " + name)


def imports(output, provider=False):
    names = [s.strip().lower() for s in re.findall(r"DLL Name:([^\r\n]+)", output.decode())]
    if not names or "msvcrt.dll" not in names:
        raise RuntimeError("PE does not import the selected MSVCRT ABI: " + repr(names))
    if any(n == "ucrtbase.dll" or n.startswith("api-ms-win-crt-") for n in names):
        raise RuntimeError("UCRT import conflicts with the Windows GNU target: " + repr(names))
    if provider and [n for n in names if "lmdb" in n] != ["ulmdb_c.dll"]:
        raise RuntimeError("wrong provider import: " + repr(names))
    return names


def exit_pass(actual, expected):
    if actual != expected:
        raise RuntimeError("unexpected exit " + str(actual) + "; expected " + str(expected))


class Run:
    def __init__(self, output, job_factory=Job):
        self.out = output.resolve()
        self.out.mkdir(parents=True, exist_ok=False)
        self.jobs = job_factory
        self.env = dict(os.environ)
        self.receipt = {"schema": 1, "complete": False, "passed": False,
                        "scope": "native Windows GNU/MSVCRT targeted build/runtime; not full parity or power-loss proof",
                        "commands": [], "toolchain": TOOLCHAIN}
        self.save()

    def save(self):
        (self.out / "receipt.json").write_text(json.dumps(self.receipt, indent=2) + "\n")

    def command(self, label, argv, data=None, expected=0, timeout=180, env=None):
        stdout, stderr = self.out / (label + ".stdout"), self.out / (label + ".stderr")
        row = {"label": label, "argv": [str(x) for x in argv], "expected_exit_code": expected, "terminal": False}
        self.receipt["commands"].append(row)
        process = job = None
        error = None
        try:
            job = self.jobs()
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen(row["argv"], cwd=ROOT, env=env or self.env,
                    stdin=subprocess.PIPE if data is not None else subprocess.DEVNULL,
                    stdout=out, stderr=err, creationflags=SUSPENDED_GROUP)
                row["pid"] = process.pid
                job.attach_and_resume(process)
                process.communicate(data, timeout=timeout)
        except BaseException as exc:
            error = exc
        finally:
            # Close even on normal leader exit: a surviving descendant still
            # belongs to this command, never to a later unrelated session.
            try:
                if job:
                    job.close()
            except BaseException as exc:
                error = error or exc
            if process:
                if process.poll() is None:
                    process.kill()  # also handles failed pre-assignment child
                try:
                    process.wait(timeout=10)
                except BaseException as exc:
                    error = error or exc
                row.update(terminal=process.poll() is not None, exit_code=process.returncode)
            for kind, path in (("stdout", stdout), ("stderr", stderr)):
                if path.exists():
                    row[kind] = {"path": path.name, "bytes": path.stat().st_size, "sha256": sha(path)}
            if error:
                row["error"] = repr(error)
            self.save()
        if error:
            raise error
        exit_pass(process.returncode, expected)
        if stdout.stat().st_size > 8 << 20:
            raise RuntimeError(label + ": unexpected captured output size")
        return stdout.read_bytes()

    def pipe(self, producer, consumer):
        """Real binary pipe; both independently owned Jobs share one deadline."""
        children = []
        error = None
        deadline = time.monotonic() + 30
        try:
            for label, argv in (("pipe-dump", producer), ("pipe-load", consumer)):
                row = {"label": label, "argv": [str(x) for x in argv], "expected_exit_code": 0, "terminal": False}
                self.receipt["commands"].append(row)
                job = self.jobs()
                entry = [row, job, None, []]
                children.append(entry)
                err = (self.out / (label + ".stderr")).open("wb")
                entry[3].append(err)
                if len(children) == 1:
                    stdin, stdout = subprocess.DEVNULL, subprocess.PIPE
                else:
                    stdin = children[0][2].stdout
                    stdout = (self.out / (label + ".stdout")).open("wb")
                    entry[3].append(stdout)
                p = subprocess.Popen(row["argv"], cwd=ROOT, env=self.env, stdin=stdin,
                                     stdout=stdout, stderr=err, creationflags=SUSPENDED_GROUP)
                entry[2] = p
                row["pid"] = p.pid
                job.attach_and_resume(p)
            children[0][2].stdout.close()
            for row, job, process, files in reversed(children):
                process.wait(timeout=max(0.001, deadline - time.monotonic()))
                if process.returncode:
                    raise RuntimeError(row["label"] + ": exit " + str(process.returncode))
        except BaseException as exc:
            error = exc
        finally:
            for row, job, process, files in reversed(children):
                try:
                    job.close()
                except BaseException as exc:
                    error = error or exc
                if process:
                    try:
                        if process.poll() is None:
                            process.kill()
                        process.wait(timeout=10)
                    except BaseException as exc:
                        error = error or exc
                    if process.stdout:
                        process.stdout.close()
                    row.update(terminal=process.poll() is not None, exit_code=process.returncode)
                for f in files:
                    f.close()
                for kind in ("stdout", "stderr"):
                    path = self.out / (row["label"] + "." + kind)
                    if path.exists():
                        row[kind] = {"path": path.name, "bytes": path.stat().st_size, "sha256": sha(path)}
                if error:
                    row["error"] = repr(error)
            self.save()
        if error:
            raise error


def download(url, path, size, digest):
    # Bound before extraction, never execute a partial or merely named download.
    with urllib.request.urlopen(url, timeout=60) as source, path.open("xb") as target:
        count = 0
        while block := source.read(min(1024 * 1024, size + 1 - count)):
            count += len(block)
            if count > size:
                raise RuntimeError("oversized download: " + url)
            target.write(block)
    checked_file(path, digest, size)


def references(run):
    result = {}
    for role in ("modern", "legacy"):
        meta = json.loads((HERE / (role + "-source.json")).read_text())
        directory = run.out / (role + "-source")
        directory.mkdir()
        for entry in meta["files"]:
            download("https://raw.githubusercontent.com/LMDB/lmdb/" + meta["commit"] +
                     "/libraries/liblmdb/" + entry["path"], directory / entry["path"],
                     entry["bytes"], entry["sha256"])
        result[role] = directory
    run.receipt["reference_inputs"] = {str(p.relative_to(run.out)): sha(p)
        for directory in result.values() for p in sorted(directory.iterdir())}
    return result


TESTS = [
    ("ulmdb-platform", "--lib", "tests::real_byte_range_locks_conflict_and_release_at_high_offsets"),
    ("ulmdb-platform", "--lib", "tests::identity_uses_real_object_id_across_hardlinks_and_directory_handles"),
    ("ulmdb-platform", "--lib", "tests::native_handle_width_and_duplicate_stream_offset_ownership"),
    ("ulmdb-storage", "fixed_address_windows", "placeholder_views_keep_exact_borrows_and_reuse_writable_addresses"),
    ("ulmdb", "fixed_portable_smoke", "portable_fixed_plain_mapped_old_borrow_and_reopen"),
    ("ulmdb", "native_cli_readers", "file_cli_reads_and_copies_keep_the_real_writer_fenced"),
    ("ulmdb", "file_capture", "metadata_capture_defers_admission_and_preserves_exact_d_and_v_during_choice_changes"),
    ("ulmdb", "resident_capture_identity", "negotiated_capture_binds_actual_platform_identity_and_retains_old_bytes"),
    ("ulmdb", "resident_export", "pinned_complete_export_survives_source_writes_collection_owner_drop_and_removal"),
    ("ulmdb", "windows_durability", "async_view_flush_preserves_debt_and_old_borrows_until_a_real_full_publication"),
]


def operator_checks(run, cc, archive, package, refs):
    binpath = package / "bin"
    tool = binpath / "ulmdb-tool.exe"
    apps = run.out / "apps"
    apps.mkdir()
    for name in ("compatibility", "physical_stats"):
        exe = apps / (name + ".exe")
        run.command(name + "-link", [cc, "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
            "-I" + str(ROOT / "clients/c/include"), ROOT / "clients/c/tests" / (name + ".c"),
            archive, "-o", exe])
        run.receipt.setdefault("public_app_bytes", {})[str(exe.relative_to(run.out))] = sha(exe)
        run.receipt.setdefault("pe_imports", {})[name] = imports(
            run.command(name + "-imports", [cc.with_name("objdump.exe"), "-p", exe]), provider=True)
        output = run.command(name, [binpath / "convert-lmdb-to-ulmdb.exe", "--abi", "1.0.2", exe,
                                   run.out / (name + "-db")])
        if (name.replace("_", "-") + "-ok").encode() not in output:
            raise RuntimeError("public C fixture did not complete: " + name)
    payload = source_module(HERE / "test_operator.py", "platform_operator_payload").PAYLOAD
    source, target = run.out / "operator-source", run.out / "operator-pipe"
    run.command("operator-load", [tool, "load", source], payload)
    original = run.command("operator-dump", [tool, "dump", source])
    run.pipe([tool, "dump", source], [tool, "load", target])
    if run.command("operator-pipe-dump", [tool, "dump", target]) != original:
        raise RuntimeError("binary pipe roundtrip changed rows/flags")
    if run.command("operator-empty", [tool, "get", target, "00ff80"]) != b"\n":
        raise RuntimeError("empty value changed")
    for verb in ("stat", "check"):
        output = json.loads(run.command("operator-" + verb, [tool, verb, "--json", target]))
        if output["schema"] != 1 or output["structural_check"] != (verb == "check"):
            raise RuntimeError("operator JSON/check contract")
    for name, options in (("raw", []), ("compact", ["--compact"])):
        copy = run.out / ("operator-" + name)
        run.command("operator-copy-" + name, [tool, "copy", *options, target, copy])
        if run.command("operator-copy-" + name + "-dump", [tool, "dump", copy]) != original:
            raise RuntimeError("copy changed data/flags")
    for i, malformed in enumerate((payload.rsplit(b"DUMP=END", 1)[0], payload + b"tail\n",
                                   payload.replace(b"DB=00000000:", b"DB=ffffffff:"))):
        path = run.out / ("malformed-" + str(i))
        run.command("malformed-" + str(i), [tool, "load", path], malformed, expected=1)
        if path.exists() or list(run.out.glob(path.name + ".staging-*")):
            raise RuntimeError("malformed stream published/leaked target")
    # Reuse the real pinned-source producer, not a Native-created substitute.
    upstream = run.out / "pinned-empty-source"
    upstream.mkdir()
    seed = apps / "migration-empty.exe"
    run.command("migration-seed-link", [cc, "-std=c11", "-O2", "-D_GNU_SOURCE",
        "-I" + str(refs["modern"]), HERE / "tests/migration_empty_name.c",
        refs["modern"] / "mdb.c", refs["modern"] / "midl.c", "-o", seed])
    run.receipt["public_app_bytes"][str(seed.relative_to(run.out))] = sha(seed)
    run.command("migration-seed", [seed, upstream])
    input_hash = sha(upstream / "data.mdb")
    migrated = run.out / "migrated"
    run.command("migrate", [binpath / "ulmdb-migrate.exe", upstream, migrated,
        "--library", binpath / "ulmdb_c.dll", "--exporter", binpath / "lmdb-export.exe"])
    for label, options, value in (("main", [], b"4d\n"), ("empty", ["-s", ""], b"45\n"),
                                  ("long", ["-s", "n" * 600], b"4c\n")):
        if run.command("migrated-" + label, [tool, "get", *options, migrated, "6b"]) != value:
            raise RuntimeError("migration MAIN/named identity changed: " + label)
    checked_file(upstream / "data.mdb", input_hash)
    for path, digest in run.receipt["public_app_bytes"].items():
        checked_file(run.out / path, digest)
    run.receipt["migration_source_data_sha256"] = input_hash


def execute(run):
    require_windows()
    run.receipt.update(host={"system": platform.platform(), "python": sys.version}, source_before=sources())
    if not run.receipt["source_before"]:
        raise RuntimeError("empty source inventory")
    run.command("runner-controls", [sys.executable, Path(__file__).with_name("test_windows.py"), "-v"])
    asset = run.out / "w64devkit-x64-2.11.0.7z.exe"
    download(TOOLCHAIN["url"], asset, TOOLCHAIN["bytes"], TOOLCHAIN["sha256"])
    extracted = run.out / "compiler"
    run.command("extract", [asset, "-y", "-o" + str(extracted)])
    cc = extracted / "w64devkit/bin/gcc.exe"
    if not cc.is_file():
        raise RuntimeError("pinned compiler layout mismatch")
    run.env["PATH"] = str(cc.parent) + os.pathsep + os.environ["PATH"]
    for key in list(run.env):
        if key.upper().startswith("ULMDB_") or key.upper() in {
            "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
            "RUSTC", "RUSTDOC", "CARGO_BUILD_TARGET", "LD_LIBRARY_PATH", "CFLAGS", "CPPFLAGS", "LDFLAGS"}:
            run.env.pop(key)
    run.env.update(CARGO_TARGET_DIR=str(run.out / "cargo-target"),
        CARGO_TERM_COLOR="never",
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=str(cc), CC=str(cc),
        CC_x86_64_pc_windows_gnu=str(cc), AR_x86_64_pc_windows_gnu=str(cc.with_name("ar.exe")))
    if run.command("compiler-target", [cc, "-dumpmachine"]).strip() != b"x86_64-w64-mingw32":
        raise RuntimeError("compiler target mismatch")
    run.command("compiler-version", [cc, "--version"])
    run.receipt["compiler_binaries"] = {name: sha(cc.with_name(name)) for name in ("gcc.exe", "ar.exe", "objdump.exe")}
    version = run.command("rustc-version", ["rustc", "+" + RUST, "-vV"])
    if not version.startswith(("rustc " + RUST + " ").encode()):
        raise RuntimeError("Rust version mismatch")
    refs = references(run)
    cargo = ["cargo", "+" + RUST]
    for package in ("ulmdb-c", "ulmdb"):
        run.command("build-" + package, [*cargo, "build", "--locked", "--release", "-p", package,
            "--target", TARGET], timeout=900)
    release = run.out / "cargo-target" / TARGET / "release"
    dll, archive = release / "ulmdb_c.dll", release / "libulmdb_c.dll.a"
    provider_hash = sha(dll)
    run.receipt["provider"] = {"path": str(dll), "sha256": provider_hash, "import_library_sha256": sha(archive)}
    package = run.out / "package"
    objdump = cc.with_name("objdump.exe")
    run.command("package", [sys.executable, HERE / "build_windows_tools.py", "--cc", cc,
        "--objdump", objdump, "--modern-source", refs["modern"], "--legacy-source", refs["legacy"],
        "--dll", dll, "--import-library", archive, "--output", package], timeout=900)
    run.command("vector-build", [sys.executable, HERE / "build_windows_operator.py", "--cc", cc,
        "--objdump", objdump, "--dll", dll, "--import-library", archive,
        "--output", run.out / "vectors", "--vectors"], timeout=600)
    run.command("windows-spool-vector", [run.out / "vectors/bin/operator-windows-vectors.exe",
                                         run.out / "private-spool-\u03bb"])
    for i, (crate, binary, name) in enumerate(TESTS):
        selection = ["--lib"] if binary == "--lib" else ["--test", binary]
        command = [*cargo, "test", "--locked", "-p", crate, "--target", TARGET, *selection, name, "--", "--exact"]
        listed = run.command("native-list-" + str(i), [*command, "--list"], timeout=900)
        if listed.decode().splitlines() != [name + ": test", "", "1 test, 0 benchmarks"]:
            raise RuntimeError("exact native test inventory changed: " + repr(listed))
        # Only one test is selected. The reporting mode with two workers keeps
        # its result line intact after nested output, even on a one-CPU host.
        result = run.command("native-" + str(i), [*command, "--test-threads=2"], timeout=900)
        rust_pass(result, name)
    operator_checks(run, cc, archive, package, refs)
    run.receipt["pe_imports"] = run.receipt.get("pe_imports", {})
    for path in [*sorted((package / "bin").glob("*.exe")), dll]:
        run.receipt["pe_imports"][path.name] = imports(run.command("pe-" + path.name, [objdump, "-p", path]))
    for alias in ("ulmdb_c.dll", "lmdb.dll", "liblmdb.dll", "libulmdb_c.dll"):
        checked_file(package / "bin" / alias, provider_hash)
    checked_file(dll, provider_hash)
    run.receipt["source_after"] = sources()
    if run.receipt["source_after"] != run.receipt["source_before"]:
        raise RuntimeError("source inventory changed during host build/runtime")
    for key, digest in run.receipt["reference_inputs"].items():
        checked_file(run.out / key, digest)
    run.receipt["artifacts"] = {str(p.relative_to(run.out)): sha(p) for base in
        (package, run.out / "vectors", run.out / "apps", release,
         run.out / "cargo-target" / TARGET / "debug") for p in sorted(base.rglob("*"))
        if p.is_file() and p.suffix.lower() in (".exe", ".dll", ".a", ".json")}
    run.receipt.update(complete=True, passed=True, exact_native_tests=[t[2] for t in TESTS])


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--output", type=Path, required=True)
    args = ap.parse_args()
    run = Run(args.output)
    try:
        execute(run)
    except BaseException as error:
        run.receipt["error"] = repr(error)
        raise
    finally:
        run.save()


if __name__ == "__main__":
    main()
