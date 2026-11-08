#!/usr/bin/env python3
"""Compile one unchanged C scenario against ULMDB and pinned upstream LMDB.

Run from any directory. Requires Python >=3.11, a C compiler, Cargo, and the
vendored pinned LMDB source in support/lmdb/vendor. No download
or installation is performed. All scenarios use separate temporary directories.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, timeout=120, **kwargs)


def pinned_lmdb(source: Path) -> Path:
    if (source / "libraries/liblmdb").is_dir():
        source = source / "libraries/liblmdb"
    pins = tomllib.loads((ROOT / "support/lmdb/source.toml").read_text())
    pin = next(item for item in pins["sources"] if item["id"] == "LMDB")
    for item in pin["files"]:
        path = source / Path(item["path"]).name
        if path.is_symlink() or not path.is_file():
            raise RuntimeError(f"Expected pinned regular LMDB source file: {path}")
        contents = path.read_bytes()
        if len(contents) != item["bytes"] or hashlib.sha256(contents).hexdigest() != item["sha256"]:
            raise RuntimeError(f"LMDB source does not match repository pin: {path}")
    print(f"LMDB baseline: {pin['ref']} ({pin['commit']})", flush=True)
    return source


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true", help="Use the existing ULMDB C library")
    parser.add_argument("--library", type=Path, default=None, help="ULMDB shared library (default: target/debug/libulmdb_c.so)")
    parser.add_argument(
        "--lmdb-source", type=Path,
        default=Path(os.environ.get("ULMDB_LMDB_SOURCE", ROOT / "support/lmdb/vendor")),
        help="Pinned LMDB checkout or its libraries/liblmdb directory",
    )
    args = parser.parse_args()
    source = pinned_lmdb(args.lmdb_source.resolve())
    if not args.no_build:
        run(["cargo", "build", "--locked", "-p", "ulmdb-c"], cwd=ROOT)
    library = (args.library or ROOT / "target/debug/libulmdb_c.so").resolve()
    if not library.is_file():
        raise RuntimeError(f"ULMDB shared library is missing: {library}")
    compiler = shlex.split(os.environ.get("CC", "cc"))
    flags = ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-pthread"]
    with tempfile.TemporaryDirectory(prefix="ulmdb-c-compliance-") as temporary:
        work = Path(temporary)
        upstream = work / "lmdb"
        candidate = work / "ulmdb"
        extensions = work / "extensions"
        lifetimes = work / "lifetimes"
        # Build upstream sources without promoting their legacy warnings to errors.
        objects = []
        for name in ["mdb", "midl"]:
            obj = work / f"{name}.o"
            run(compiler + ["-std=c11", "-O2", "-pthread", "-D_GNU_SOURCE", "-I", str(source), "-c", str(source / f"{name}.c"), "-o", str(obj)])
            objects.append(str(obj))
        run(compiler + flags + ["-I", str(source), str(HERE / "compatibility.c"), *objects, "-o", str(upstream)])
        candidate_link = [str(library), f"-Wl,-rpath,{library.parent}"]
        run(compiler + flags + ["-I", str(ROOT / "clients/c/include"), str(HERE / "compatibility.c"), *candidate_link, "-o", str(candidate)])
        run(compiler + flags + ["-I", str(ROOT / "clients/c/include"), str(HERE / "extensions.c"), *candidate_link, "-o", str(extensions)])
        run(compiler + flags + ["-I", str(ROOT / "clients/c/include"), str(HERE / "lifetimes.c"), *candidate_link, "-o", str(lifetimes)])
        outputs = []
        for executable in [upstream, candidate]:
            database = work / f"{executable.name}-database"
            database.mkdir()
            outputs.append(run([str(executable), str(database)], capture_output=True).stdout)
        if outputs[0] != outputs[1]:
            diff = "".join(difflib.unified_diff(outputs[0].splitlines(True), outputs[1].splitlines(True), fromfile="LMDB", tofile="ULMDB"))
            raise RuntimeError(f"C compatibility outputs differ:\n{diff}")
        print(outputs[1], end="", flush=True)
        feature_outputs = []
        for label, include, link in [
            ("lmdb-features", source, objects),
            ("ulmdb-features", ROOT / "clients/c/include", candidate_link),
        ]:
            executable = work / label
            run(compiler + flags + ["-I", str(include), str(HERE / "features.c"), *link, "-o", str(executable)])
            database = work / f"{label}-database"
            database.mkdir()
            feature_outputs.append(run([str(executable), str(database)], capture_output=True).stdout)
        if feature_outputs[0] != feature_outputs[1]:
            raise RuntimeError(f"Feature outputs differ: {feature_outputs!r}")
        print(feature_outputs[1], end="", flush=True)
        reserve_outputs = []
        for label, include, link in [
            ("lmdb-reserve", source, objects),
            ("ulmdb-reserve", ROOT / "clients/c/include", candidate_link),
        ]:
            executable = work / label
            run(compiler + flags + ["-I", str(include), str(HERE / "reserve.c"), *link, "-o", str(executable)])
            database = work / f"{label}-database"
            database.mkdir()
            reserve_outputs.append(run([str(executable), str(database)], capture_output=True).stdout)
        if reserve_outputs[0] != reserve_outputs[1]:
            raise RuntimeError(f"Reserve outputs differ: {reserve_outputs!r}")
        print(reserve_outputs[1], end="", flush=True)
        deletion_outputs = []
        for label, include, link in [
            ("lmdb-cursor-delete", source, objects),
            ("ulmdb-cursor-delete", ROOT / "clients/c/include", candidate_link),
        ]:
            executable = work / label
            run(compiler + flags + ["-I", str(include), str(HERE / "cursor_delete.c"), *link, "-o", str(executable)])
            database = work / f"{label}-database"
            database.mkdir()
            deletion_outputs.append(run([str(executable), str(database)], capture_output=True).stdout)
        if deletion_outputs[0] != deletion_outputs[1]:
            diff = "".join(difflib.unified_diff(deletion_outputs[0].splitlines(True), deletion_outputs[1].splitlines(True), fromfile="LMDB", tofile="ULMDB"))
            raise RuntimeError(f"Cursor deletion outputs differ:\n{diff}")
        print("cursor-delete-ok", flush=True)
        drop_outputs = []
        for label, include, link in [
            ("lmdb-drop", source, objects),
            ("ulmdb-drop", ROOT / "clients/c/include", candidate_link),
        ]:
            executable = work / label
            run(compiler + flags + ["-I", str(include), str(HERE / "drop.c"), *link, "-o", str(executable)])
            database = work / f"{label}-database"
            database.mkdir()
            drop_outputs.append(run([str(executable), str(database)], capture_output=True).stdout)
        if drop_outputs[0] != drop_outputs[1]:
            raise RuntimeError(f"Database clear outputs differ: {drop_outputs!r}")
        print(drop_outputs[1], end="", flush=True)
        database = work / "extensions-database"
        database.mkdir()
        run([str(extensions), str(database)])
        lifetime_a, lifetime_b = work / "lifetime-a", work / "lifetime-b"
        lifetime_a.mkdir()
        lifetime_b.mkdir()
        run([str(lifetimes), str(lifetime_a), str(lifetime_b)])
        copies = work / "copies"
        run(compiler + flags + ["-I", str(ROOT / "clients/c/include"), str(HERE / "copies.c"), *candidate_link, "-o", str(copies)])
        run([str(copies), str(work / "copy-source"), str(work / "copy-normal"), str(work / "copy-compact"), str(work / "copy-archive")])
        controls = work / "controls"
        run(compiler + flags + ["-I", str(ROOT / "clients/c/include"), str(HERE / "controls.c"), *candidate_link, "-o", str(controls)])
        run([str(controls), str(work / "controls-source"), str(work / "controls-file")])
    print("PASS: unchanged C source matched pinned LMDB; native IPv4/IPv6 and lifecycle tests passed")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        if error.stdout:
            print(error.stdout, file=sys.stderr, end="")
        if error.stderr:
            print(error.stderr, file=sys.stderr, end="")
        raise SystemExit(error.returncode) from error
    except (OSError, RuntimeError, StopIteration) as error:
        raise SystemExit(str(error)) from error
