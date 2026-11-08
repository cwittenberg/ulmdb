"""Freeze the dirty working source without changing the checkout or its reports."""
from pathlib import Path
import hashlib
import os
import platform
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_files(root):
    files = [root / p for p in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
                                "benchmark/run.py", "benchmark/run.sh", "benchmark/report.py", "benchmark/requirements.txt")]
    for name in ("crates", "clients/c", "clients/rust", "benchmark/support/lmdb",
                 "benchmark/engine", "benchmark/millions", "benchmark/lookup-strategies"):
        for p in (root / name).rglob("*"):
            if not p.is_file() or {"target", "__pycache__", "results"}.intersection(p.relative_to(root).parts):
                continue
            if p.suffix in {".rs", ".toml", ".lock", ".py", ".c", ".h"} or p.name in {"LICENSE", "COPYRIGHT"}:
                files.append(p)
    return sorted(set(files))


def freeze(root, destination):
    manifest = {}
    for p in source_files(root):
        relative = p.relative_to(root)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        contents = p.read_bytes()
        target.write_bytes(contents)
        manifest[str(relative)] = hashlib.sha256(contents).hexdigest()
    # A changing source must never be silently captured as a single generation.
    assert manifest == {str(p.relative_to(root)): sha(p) for p in source_files(root)}, "source changed while freezing"
    return manifest


def verify_frozen(root, manifest):
    assert all(sha(root / p) == value for p, value in manifest.items()), "frozen source changed"


def environment():
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ULMDB_BENCH_", "ULMDB_TEST_", "ULMDB_RESIDENT_", "ULMDB_HYBRID_"))}
    for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        env.pop(key, None)
    for key in list(env):
        if key.startswith("CARGO_PROFILE_") or key in {"RUSTC", "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_TARGET"} or (
            key.startswith("CARGO_TARGET_") and key.endswith(("_RUSTFLAGS", "_LINKER"))
        ):
            env.pop(key, None)
    env["CARGO_BUILD_JOBS"] = "2"
    return env


def machine(cpu):
    model = next(s.split(":", 1)[1].strip() for s in Path("/proc/cpuinfo").read_text().splitlines()
                 if s.startswith("model name"))
    return {"cpu_model": model, "architecture": platform.machine(), "kernel": platform.release(),
            "cpu": cpu, "allowed_cpus": sorted(os.sched_getaffinity(0)),
            "rustc": subprocess.check_output(["rustc", "+1.94.0", "-Vv"], text=True),
            "memory_total_kib": int(Path("/proc/meminfo").read_text().splitlines()[0].split()[1]),
            "exclusive_host": False}
