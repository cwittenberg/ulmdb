#!/usr/bin/env python3
"""Compile and run the C adapter's bounded cursor memory/lifetime regression."""
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
suffix = ".dylib" if sys.platform == "darwin" else ".so"
library = Path(os.environ.get("ULMDB_LIBRARY", str(ROOT / "target/debug" / f"libulmdb_c{suffix}"))).resolve()
compiler = shlex.split(os.environ.get("CC", "cc"))
with tempfile.TemporaryDirectory(prefix="ulmdb-streaming-") as temporary:
    directory = Path(temporary)
    executable = directory / "streaming"
    command = compiler + ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-I", str(ROOT / "clients/c/include"), str(HERE / "streaming.c"), str(library), f"-Wl,-rpath,{library.parent}", "-o", str(executable)]
    subprocess.run(command, check=True, timeout=60)
    database = directory / "database"
    subprocess.run([str(executable), "seed", str(database)], check=True, timeout=60)
    subprocess.run([str(executable), "scan", str(database)], check=True, timeout=60)
