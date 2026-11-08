#!/usr/bin/env python3
"""Run the Linux LMDB qualification gate, preserving logs and fresh receipts.

Use --bootstrap to fetch hash-pinned reference sources and prepare an isolated
Python environment. No system packages are installed. Ordinary runs are offline
apart from dependencies fetched by the existing PHP Docker client test.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import sysconfig
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PREP = ROOT / "target/qualification"
VENV = PREP / "venv"
REFERENCE = PREP / "reference-0.9.31/lib"
PYTHON = VENV / "bin/python"
LMDB_URL = "https://files.pythonhosted.org/packages/1f/80/24a4064047b1c98ac73c0ad806f16bcd2d800c2eb460d6e6c2961034b8a1/lmdb-3.0.0.tar.gz"
LMDB_SHA256 = "06dda0723545e14d56ac7dcd6f582d9922c2ed5e16f1c3f8d0e670a96ec2ee65"
TOOLS = ["pip==25.0.1", "setuptools==75.8.2", "wheel==0.45.1", "cmake==3.31.6"]
SOURCES = {
    "legacy": (HERE / "legacy-source.json", Path.home() / ".cache/ulmdb-compat/lmdb-0.9.31"),
    "modern": (HERE / "modern-source.json", ROOT / "benchmark/support/lmdb/vendor"),
    "current09": (HERE / "current09-source.json", Path.home() / ".cache/ulmdb-compat/lmdb-0.9.36"),
}



_ACTIVE_STAGES = {}


def process_identity(pid):
    try:
        tail = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    except FileNotFoundError:
        return None
    return {"pid": pid, "state": tail[0], "pgid": int(tail[2]),
            "sid": int(tail[3]), "starttime": int(tail[19])}


def owned_anchor(process):
    expected = process._qualification_identity
    current = process_identity(process.pid)
    require(current is not None and all(current[key] == expected[key]
            for key in ("pid", "pgid", "sid", "starttime")),
            "lost unreaped stage identity; refuse signaling an unproven process group")
    return current


def live_group_members(process):
    owned_anchor(process)
    identity = process._qualification_identity
    members = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            item = process_identity(int(path.name))
        except (ProcessLookupError, PermissionError):
            continue
        if item and item["pgid"] == identity["pgid"] and item["sid"] == identity["sid"] and item["state"] not in {"Z", "X"}:
            members.append(item)
    return members


def wait_stage(process, timeout):
    # Never reap here: the zombie leader reserves its PID/session identity
    # until every owned same-group descendant has stopped.
    deadline = time.monotonic() + timeout
    while True:
        result = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
        if result is not None:
            return result.si_status if result.si_code == os.CLD_EXITED else -result.si_status
        if time.monotonic() >= deadline:
            raise subprocess.TimeoutExpired(process.args, timeout)
        time.sleep(min(0.05, max(0, deadline - time.monotonic())))


def install_interrupt_handlers():
    def interrupted(signum, frame):
        raise KeyboardInterrupt("qualification interrupted by signal " + str(signum))
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, interrupted)


@contextmanager
def ignore_interrupts():
    previous = {sig: signal.signal(sig, signal.SIG_IGN) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        yield
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def start_stage(command, **kwargs):
    pending = []
    def defer(signum, frame):
        pending.append(signum)
    previous = {sig: signal.signal(sig, defer) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        process = subprocess.Popen(command, start_new_session=True, **kwargs)
        identity = process_identity(process.pid)
        require(identity is not None and identity["pgid"] == process.pid and identity["sid"] == process.pid,
                "new stage did not establish its own process group/session")
        process._qualification_identity = identity
        _ACTIVE_STAGES[process.pid] = process
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    if pending:
        raise KeyboardInterrupt("qualification interrupted during stage creation")
    return process


def stop_stages():
    records = []
    with ignore_interrupts():
        for process in list(_ACTIVE_STAGES.values()):
            owned_anchor(process)
            record = {"identity": process._qualification_identity, "before": live_group_members(process)}
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            # The unreaped leader may already have exited. Waiting only for it
            # would miss live descendants, including ones ignoring TERM.
            deadline = time.monotonic() + 5
            while live_group_members(process) and time.monotonic() < deadline:
                time.sleep(0.05)
            if live_group_members(process):
                owned_anchor(process)
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                record["escalated_to_kill"] = True
                deadline = time.monotonic() + 2
                while live_group_members(process) and time.monotonic() < deadline:
                    time.sleep(0.02)
            record["after"] = live_group_members(process)
            require(not record["after"], "owned stage group still has live members after bounded cleanup")
            # Reap only now, while the same-session group has no live members.
            record["returncode"] = process.wait(timeout=2)
            _ACTIVE_STAGES.pop(process.pid, None)
            records.append(record)
    return records


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def utc():
    return datetime.now(timezone.utc).isoformat()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class Qualification:
    def __init__(self, directory, timeout, prepare_only):
        self.directory = directory
        directory.mkdir(parents=True, exist_ok=False)
        self.timeout = timeout
        self.env = os.environ.copy()
        for key in ("LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "ULMDB_LIBRARY",
                    "ULMDB_MIGRATOR", "ULMDB_LMDB_ABI"):
            self.env.pop(key, None)
        self.env["PATH"] = str(VENV / "bin") + os.pathsep + self.env.get("PATH", "")
        self.env["PYTHONDONTWRITEBYTECODE"] = "1"
        self.env["ULMDB_LMDB_SOURCE"] = str(SOURCES["modern"][1])
        self.receipt = {"schema": 1, "profile": "lmdb-linux-release-qualification-v1",
                        "scope": "preparation" if prepare_only else "workspace-and-linux-clients",
                        "started_utc": utc(), "complete": False, "passed": False, "steps": []}
        self.save()

    def save(self):
        (self.directory / "receipt.json").write_text(json.dumps(self.receipt, indent=2) + "\n")

    def run(self, name, command, env=None):
        command = [str(item) for item in command]
        log = self.directory / f"{len(self.receipt['steps']) + 1:02}-{name}.log"
        step = {"name": name, "command": command, "started_utc": utc(), "log": str(log)}
        self.receipt["steps"].append(step)
        self.save()
        print(f"RUN {name}: {shlex.join(command)}", flush=True)
        start = time.monotonic()
        process = None
        try:
            with log.open("w") as output:
                process = start_stage(command, cwd=ROOT, env=env or self.env, stdout=output,
                                      stderr=subprocess.STDOUT)
                try:
                    result = wait_stage(process, self.timeout)
                except BaseException:
                    # Only this stage's process group is owned by the gate.
                    # Stop descendants too, so a failed deadline cannot leave
                    # tests modifying shared artifacts after the gate exits.
                    step.setdefault("owned_cleanup", []).extend(stop_stages())
                    raise
            step["exit_code"] = result
            require(result == 0, f"{name} failed ({result}); see {log}")
            step.setdefault("owned_cleanup", []).extend(stop_stages())
        except BaseException:
            step.setdefault("owned_cleanup", []).extend(stop_stages())
            if log.exists():
                print("\n".join(log.read_text(errors="replace").splitlines()[-40:]), file=sys.stderr)
            raise
        finally:
            step["elapsed_seconds"] = round(time.monotonic() - start, 3)
            step["finished_utc"] = utc()
            if log.exists():
                step["log_sha256"] = digest(log)
            self.save()
        print(f"PASS {name} ({step['elapsed_seconds']}s)", flush=True)


def download(url, path, expected):
    require(not path.is_symlink(), f"refuse symlink dependency: {path}")
    if path.exists():
        require(digest(path) == expected, f"dependency hash mismatch; preserve and investigate {path}")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    # Validate before publishing: a failed fetch never becomes a selected source.
    with urllib.request.urlopen(url, timeout=60) as response:
        contents = response.read()
    require(hashlib.sha256(contents).hexdigest() == expected, f"download hash mismatch: {url}")
    with path.open("xb") as output:
        output.write(contents)


def reference_sources(bootstrap):
    result = {}
    for name, (manifest, directory) in SOURCES.items():
        pin = json.loads(manifest.read_text())
        files = {}
        for entry in pin["files"]:
            path = directory / entry["path"]
            if not path.exists() and bootstrap and name != "modern":
                url = f"https://raw.githubusercontent.com/LMDB/lmdb/{pin['commit']}/libraries/liblmdb/{entry['path']}"
                download(url, path, entry["sha256"])
            require(path.is_file() and not path.is_symlink(),
                    f"missing pinned reference file {path}; run with --bootstrap")
            require(path.stat().st_size == entry["bytes"] and digest(path) == entry["sha256"],
                    f"reference source differs from pin: {path}; no files were overwritten")
            files[entry["path"]] = entry["sha256"]
        result[name] = {"ref": pin["ref"], "commit": pin["commit"], "directory": str(directory),
                        "manifest_sha256": digest(manifest), "files": files}
    return result


def tool_prerequisites(env):
    require(sys.platform == "linux", "this qualification profile requires a Linux host")
    require(sys.version_info >= (3, 11), "Python 3.11 or newer is required")
    require((Path(sysconfig.get_path("include")) / "Python.h").is_file(),
            "Python development headers are required to build the public python-lmdb extension")
    for command in ("cargo", "rustc", "make", "cc", "c++", "node", "npm", "git", "ldd", "nm", "ld", "objcopy", "readelf", "strace"):
        require(shutil.which(command, path=env["PATH"]), f"missing {command}; install host prerequisites explicitly")
    require(not env.get("CARGO_TARGET_DIR") or Path(env["CARGO_TARGET_DIR"]).resolve() == ROOT / "target",
            "client scripts require the repository target directory; unset CARGO_TARGET_DIR")
    node = shutil.which("node", path=env["PATH"])
    headers = [Path(env["NODE_INCLUDE_DIR"])] if env.get("NODE_INCLUDE_DIR") else []
    headers += [Path(node).resolve().parent.parent / "include/node", Path("/usr/include/node"), Path("/usr/local/include/node")]
    require(any((path / "node_api.h").is_file() for path in headers),
            "Node development headers are required; set NODE_INCLUDE_DIR explicitly if necessary")
    if "clients/php/test-docker.sh" in (ROOT / "clients/test.sh").read_text():
        require(shutil.which("docker", path=env["PATH"]), "clients/test.sh requires Docker for the PHP client")
        probe = subprocess.run(["docker", "info"], env=env, capture_output=True, timeout=20)
        require(probe.returncode == 0, "Docker daemon is unavailable or inaccessible; required PHP client tests cannot run")


def bootstrap_python(gate):
    legacy = SOURCES["legacy"][1]
    REFERENCE.mkdir(parents=True, exist_ok=True)
    gate.run("python-reference-library", shlex.split(gate.env.get("CC", "cc")) + [
        "-std=c11", "-O2", "-shared", "-fPIC", "-pthread", "-D_GNU_SOURCE",
        "-Wl,-soname,liblmdb.so.0", "-I", legacy, legacy / "mdb.c", legacy / "midl.c",
        "-o", REFERENCE / "liblmdb.so.0"])
    alias = REFERENCE / "liblmdb.so"
    if alias.is_symlink():
        require(os.readlink(alias) == "liblmdb.so.0", f"unexpected dependency alias: {alias}")
    else:
        require(not alias.exists(), f"refuse replacing dependency: {alias}")
        alias.symlink_to("liblmdb.so.0")
    if not PYTHON.exists():
        gate.run("isolated-python", [sys.executable, "-m", "venv", "--without-pip", VENV])
    # Host pip's --python mode installs only into this venv, including on hosts
    # whose python3-venv package omits ensurepip. It never installs into system Python.
    probe = subprocess.run([sys.executable, "-m", "pip", "--version"], capture_output=True)
    if probe.returncode == 0:
        pip = [sys.executable, "-m", "pip", "--python", str(PYTHON)]
    else:
        gate.run("venv-ensurepip", [PYTHON, "-m", "ensurepip", "--upgrade"])
        pip = [str(PYTHON), "-m", "pip"]
    gate.run("isolated-build-tools", pip + ["install", "--disable-pip-version-check", *TOOLS])
    archive = PREP / "downloads/lmdb-3.0.0.tar.gz"
    download(LMDB_URL, archive, LMDB_SHA256)
    build_env = {**gate.env, "LMDB_FORCE_SYSTEM": "1", "LMDB_PURE": "1",
                 "LMDB_INCLUDEDIR": str(legacy), "LMDB_LIBDIR": str(REFERENCE)}
    gate.run("public-python-lmdb", pip + ["install", "--disable-pip-version-check", "--no-deps",
             "--no-build-isolation", "--force-reinstall", str(archive)], env=build_env)


def python_prerequisites(gate):
    require(PYTHON.is_file() and (REFERENCE / "liblmdb.so.0").is_file(),
            "isolated python-lmdb/reference library missing; run with --bootstrap")
    for command in ("cmake", "ctest"):
        require(shutil.which(command, path=gate.env["PATH"]), f"missing {command}; --bootstrap prepares pinned local tools")
    env = {**gate.env, "LD_LIBRARY_PATH": str(REFERENCE)}
    probe = subprocess.run([PYTHON, "-c", "import json,lmdb,lmdb.cpython,importlib.metadata as m; print(json.dumps({'version':lmdb.__version__,'extension':lmdb.cpython.__file__,'packages':{p:m.version(p) for p in ['pip','setuptools','wheel','cmake']}}))"],
                           env=env, capture_output=True, text=True, timeout=30)
    require(probe.returncode == 0, f"isolated python-lmdb import failed; run --bootstrap: {probe.stderr.strip()}")
    info = json.loads(probe.stdout)
    require(info["version"] == "3.0.0", "qualification requires python-lmdb 3.0.0; run --bootstrap")
    expected = dict(item.split("==", 1) for item in TOOLS)
    require(info["packages"] == expected, "isolated build-tool versions differ from pins; run --bootstrap")
    linked = subprocess.run(["ldd", info["extension"]], env=env, capture_output=True, text=True, timeout=20)
    require(linked.returncode == 0 and str(REFERENCE / "liblmdb.so.0") in linked.stdout,
            "python-lmdb must link dynamically to the pinned reference, not bundle LMDB; run --bootstrap")
    info.update({"extension_sha256": digest(info["extension"]), "reference_sha256": digest(REFERENCE / "liblmdb.so.0"),
                 "source_archive_sha256": LMDB_SHA256})
    gate.receipt["public_python_lmdb"] = info
    versions = {}
    for name, command in {"rustc": ["rustc", "--version", "--verbose"], "cargo": ["cargo", "--version"],
                          "node": ["node", "--version"], "npm": ["npm", "--version"],
                          "cc": ["cc", "--version"], "cmake": ["cmake", "--version"]}.items():
        output = subprocess.check_output(command, cwd=ROOT, env=gate.env, text=True, timeout=30)
        versions[name] = output.strip()
    versions["python"] = sys.version
    gate.receipt["tool_versions"] = versions
    gate.save()


def source_fingerprint():
    result = subprocess.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                            cwd=ROOT, capture_output=True, check=True)
    paths = set(os.fsdecode(item) for item in result.stdout.split(b"\0") if item)
    selected = {}
    for item in sorted(paths):
        path = ROOT / item
        # The separate performance campaign rewrites benchmark source/reports.
        # Pinned LMDB inputs are validated independently before and after gates.
        if item.startswith("benchmark/"):
            continue
        # Include the actual compiled C++/Node client inputs, linker map,
        # Docker context policy and any embedded explorer source assets.
        if path.is_file() and (path.suffix in {".rs", ".c", ".h", ".cpp", ".hpp", ".js", ".ts", ".map",
                                               ".py", ".sh", ".json", ".toml", ".lock", ".yml", ".php"}
                               or path.name in {"Makefile", "CMakeLists.txt", "Dockerfile", ".dockerignore"}
                               or path.name.endswith(".dockerignore")
                               or item.startswith("utility/explorer/src/")):
            if "/conformance/" not in item:
                selected[item] = digest(path)
    return selected


def qualify(gate):
    before = source_fingerprint()
    gate.receipt["source_sha256"] = before
    gate.run("workspace", ["cargo", "test", "--workspace", "--locked"])
    gate.run("native-lifecycle-faults", [
        "cargo", "test", "--locked", "-p", "ulmdb", "--features", "fault-injection",
        "--test", "prepared", "--test", "prepared_faults", "--test", "rollback",
        "--test", "rollback_allocator", "--test", "rollback_faults",
        "--test", "rollback_validation",
    ])
    gate.run("native-copy-faults", [
        "cargo", "test", "--locked", "-p", "ulmdb", "--features", "fault-injection",
        "--test", "client_copy", "--test", "client_copy_faults",
        "--", "--nocapture", "--test-threads=1",
    ])
    gate.run("c-lifecycle-faults", [
        "cargo", "test", "--locked", "-p", "ulmdb-c", "--lib",
        "--features", "ulmdb/fault-injection", "--", "--ignored", "--nocapture",
        "--test-threads=1",
    ])
    gate.run("clients", ["bash", "clients/test.sh"])
    gate.run("native-runner-controls", [
        PYTHON, "clients/lmdb-ulmdb/qualification/test_native_runner.py", "-v",
    ])
    gate.run("native-file-coordination", [
        PYTHON, "clients/lmdb-ulmdb/qualification/run_native.py",
        "--run-dir", gate.directory / "native-coordination",
        "--adapter", ROOT / "target/lmdb-ulmdb/lib/liblmdb.so.0",
    ])
    for profile, label in (("common-v1", "common"), ("features-v2", "features")):
        arguments = ["--profile", profile, "--report-dir", str(gate.directory / label), "--keep-traces"]
        gate.run(profile, ["make", "-C", "clients/lmdb-ulmdb", "parity", "ARGS=--no-cargo",
                          "PARITY_ARGS=" + shlex.join(arguments)])
    overrides = ["--receipt", "common-v1=" + str(gate.directory / "common/receipt.json"),
                 "--receipt", "features-v2=" + str(gate.directory / "features/receipt.json"),
                 "--receipt", "native-coordination=" + str(gate.directory / "native-coordination/receipt.json")]
    aliases = overrides + ["--output", str(gate.directory / "FEATURE_STATUS.md")]
    registry = [PYTHON, "clients/lmdb-ulmdb/features/registry.py"]
    gate.run("feature-matrix-render", registry + ["render", *aliases])
    gate.run("feature-matrix-check", registry + ["check", *aliases])
    gate.run("feature-matrix-controls", [PYTHON, "clients/lmdb-ulmdb/features/test_registry.py", *overrides])
    gate.run("public-python-compatibility", [PYTHON, "clients/python/compatibility/test_python_lmdb.py",
             "--python", PYTHON, "--reference-lib", REFERENCE, "--package", ROOT / "target/lmdb-ulmdb"])
    gate.run("negative-controls", [PYTHON, "clients/lmdb-ulmdb/tests/negative_controls.py",
             "--output", gate.directory / "negative-controls"])
    gate.run("lifecycle-negative-controls", [PYTHON, "clients/lmdb-ulmdb/tests/negative_lifecycle_controls.py",
             "--apis", "both", "--package", ROOT / "target/lmdb-ulmdb",
             "--archive", ROOT / "target/debug/libulmdb_c.a",
             "--output", gate.directory / "lifecycle-negative-controls"])
    durability = HERE / "qualification/durability"
    control_env = {**gate.env, "ULMDB_DURABILITY_INTERRUPT_ARTIFACTS": str(gate.directory / "durability-controls")}
    gate.run("durability-harness-controls", [
        PYTHON, "-m", "unittest", "discover", "-s", durability, "-p", "test_*.py", "-v",
    ], env=control_env)
    gate.run("outer-qualification-ownership-controls", [
        PYTHON, HERE / "qualification/test_outer_gate.py", "--artifacts", gate.directory / "outer-gate-controls", "-v",
    ])
    helper = gate.directory / "durability-helper"
    gate.run("durability-empty-backend-initializer", [
        PYTHON, durability / "build_backend_init.py", "--source-root", ROOT, "--output", helper,
    ])
    gate.run("c-buffered-durability", [
        PYTHON, durability / "run.py", "--source-root", ROOT,
        "--package", ROOT / "target/lmdb-ulmdb", "--archive", ROOT / "target/debug/libulmdb_c.a",
        "--lmdb-source", SOURCES["modern"][1], "--legacy-source", SOURCES["legacy"][1],
        "--current09-source", SOURCES["current09"][1],
        "--backend-init", helper / "target/debug/c-policy-empty-backend-init",
        "--backend-init-provenance", helper / "provenance.json",
        "--output", gate.directory / "durability",
    ])
    alias_qualification = HERE / "qualification/aliases"
    gate.run("alias-runner-controls", [
        PYTHON, "-m", "unittest", "discover", "-s", alias_qualification, "-p", "test_*.py", "-v",
    ])
    gate.run("c-alias-previous", [
        PYTHON, alias_qualification / "run.py", "--source-root", ROOT,
        "--package", ROOT / "target/lmdb-ulmdb",
        "--lmdb-source", SOURCES["modern"][1], "--legacy-source", SOURCES["legacy"][1],
        "--current09-source", SOURCES["current09"][1],
        "--output", gate.directory / "aliases",
    ])
    gate.run("c-read-ahead", [
        PYTHON, ROOT / "clients/c/tests/read_ahead.py",
        "--package", ROOT / "target/lmdb-ulmdb",
        "--lmdb-source", SOURCES["modern"][1], "--legacy-source", SOURCES["legacy"][1],
        "--current09-source", SOURCES["current09"][1],
        "--backend-init", helper / "target/debug/c-policy-empty-backend-init",
        "--backend-init-provenance", helper / "provenance.json",
        "--output", gate.directory / "read-ahead",
    ])
    require(source_fingerprint() == before,
            "qualification source files changed during the run; retain logs and rerun after changes settle")
    require(reference_sources(False) == gate.receipt["references"],
            "pinned reference inputs changed during qualification")
    artifacts = [ROOT / "target/lmdb-ulmdb/lib/liblmdb.so.0", ROOT / "target/debug/libulmdb_c.a",
                 ROOT / "target/lmdb-ulmdb-parity", gate.directory / "common/receipt.json",
                 gate.directory / "features/receipt.json", gate.directory / "negative-controls/controls.json",
                 gate.directory / "lifecycle-negative-controls/receipt.json",
                 gate.directory / "native-coordination/receipt.json",
                 gate.directory / "durability-helper/provenance.json", gate.directory / "durability/receipt.json",
                 gate.directory / "aliases/receipt.json", gate.directory / "read-ahead/receipt.json"]
    gate.receipt["artifacts"] = {str(path): digest(path) for path in artifacts}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bootstrap", action="store_true", help="explicitly fetch pinned sources and install dependencies inside target/qualification/venv")
    parser.add_argument("--prepare-only", action="store_true", help="with --bootstrap, prepare prerequisites without claiming qualification")
    parser.add_argument("--run-dir", type=Path, help="new artifact directory; defaults to a unique target/qualification/runs directory")
    parser.add_argument("--stage-timeout", type=int, default=3600, help="deadline in seconds per stage (default: 3600)")
    args = parser.parse_args()
    install_interrupt_handlers()
    if args.prepare_only and not args.bootstrap:
        parser.error("--prepare-only requires --bootstrap")
    if args.stage_timeout <= 0:
        parser.error("--stage-timeout must be positive")
    directory = (args.run_dir or PREP / "runs" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + f"-{os.getpid()}")).resolve()
    gate = Qualification(directory, args.stage_timeout, args.prepare_only)
    print(f"Qualification artifacts: {directory}", flush=True)
    try:
        tool_prerequisites(gate.env)
        gate.receipt["references"] = reference_sources(args.bootstrap)
        gate.save()
        if args.bootstrap:
            bootstrap_python(gate)
        python_prerequisites(gate)
        if not args.prepare_only:
            qualify(gate)
        gate.receipt.update({"complete": True, "passed": True, "finished_utc": utc()})
        gate.save()
        print(f"PASS {'preparation only' if args.prepare_only else 'qualification'}: {directory / 'receipt.json'}", flush=True)
        return 0
    except (KeyboardInterrupt, OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        if isinstance(error, KeyboardInterrupt):
            error = RuntimeError("qualification interrupted; stage processes stopped and incomplete evidence retained")
        with ignore_interrupts():
            stop_stages()
            gate.receipt.update({"error": str(error), "finished_utc": utc()})
            gate.save()
            print(f"FAIL: {error}\nIncomplete receipt: {directory / 'receipt.json'}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
