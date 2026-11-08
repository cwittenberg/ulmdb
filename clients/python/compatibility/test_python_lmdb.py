#!/usr/bin/env python3
"""Check the public Python binding against original and adapter libraries.

The supplied Python must already have python-lmdb built against a shared LMDB
library (LMDB_FORCE_SYSTEM=1). This optional test never installs dependencies.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
APP = Path(__file__).with_name("python_lmdb_app.py")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--reference-lib", type=Path, required=True)
    parser.add_argument("--package", type=Path, default=ROOT / "target/lmdb-ulmdb")
    args = parser.parse_args()
    original_hash = hashlib.sha256(APP.read_bytes()).hexdigest()
    probe_env = os.environ.copy()
    probe_env["LD_LIBRARY_PATH"] = str(args.reference_lib.resolve())
    extension = subprocess.check_output([args.python, "-c", "import lmdb.cpython; print(lmdb.cpython.__file__)"], env=probe_env, text=True).strip()
    extension_hash = hashlib.sha256(Path(extension).read_bytes()).hexdigest()

    def run(action, path, adapter):
        env = os.environ.copy()
        env.pop("LD_PRELOAD", None)
        env["ULMDB_LMDB_ABI"] = "0.9"
        env["ULMDB_MIGRATOR"] = str(args.package.resolve() / "bin/ulmdb-migrate")
        env["LD_LIBRARY_PATH"] = str(args.package.resolve() / "lib" if adapter else args.reference_lib.resolve())
        subprocess.run([args.python, str(APP), action, str(path)], check=True, env=env, timeout=60)

    with tempfile.TemporaryDirectory(prefix="ulmdb-python-lmdb-") as temporary:
        source = Path(temporary) / "existing-lmdb"
        native = Path(temporary) / "new-ulmdb"
        run("seed", source, False)
        before = hashlib.sha256((source / "data.mdb").read_bytes()).hexdigest()
        run("verify", source, False)
        run("verify", source, True)  # Automatic sidecar import, same application.
        run("update", source, True)
        run("verify-updated", source, True)
        run("verify", source, False)  # Preserved reference files remain unchanged.
        run("seed", native, True)
        run("verify", native, True)
        assert hashlib.sha256((source / "data.mdb").read_bytes()).hexdigest() == before
        assert (source / ".ulmdb/CURRENT").is_file()
    assert hashlib.sha256(APP.read_bytes()).hexdigest() == original_hash
    assert hashlib.sha256(Path(extension).read_bytes()).hexdigest() == extension_hash
    print("PASS: unchanged python-lmdb application and extension ran against LMDB and ULMDB")


if __name__ == "__main__":
    main()
