#!/usr/bin/env python3
"""Build and run the public C incremental and transform implementation checks."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, default=ROOT / "target/lmdb-ulmdb")
    parser.add_argument("--large-values", action="store_true", help="also run the explicit65MiB resource journey")
    parser.add_argument("--jumbo-values", action="store_true", help="also run sequential257MiB ordinary/cache/WRITEMAP resource journeys")
    args = parser.parse_args()
    package = args.package.resolve()
    compiler = shlex.split(os.environ.get("CC", "cc"))
    env = dict(os.environ, ULMDB_LMDB_ABI="1.0.2")
    with tempfile.TemporaryDirectory(prefix="ulmdb-c-extensions-") as temporary:
        work = Path(temporary)

        def run(arguments):
            subprocess.run([str(arg) for arg in arguments], cwd=ROOT, env=env,
                           check=True, timeout=120)

        plugin = work / "crypto-module.so"
        run(compiler + ["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror",
                        "-I", ROOT / "clients/c/include", HERE / "crypto_module_plugin.c",
                        "-o", plugin])
        names = ["incremental", "crypto_module", "transforms", "reserve_ordering", "physical_stats", "nolock", "catalog_capacity", "catalog_map_capacity", "jumbo_preflight", "maintenance"]
        if args.large_values:
            names.append("large_values")
        if args.jumbo_values:
            names.append("jumbo_values")
        for name in names:
            executable = work / name
            run(compiler + ["-Wall", "-Wextra", "-Werror", "-I", ROOT / "clients/c/include",
                            HERE / f"{name}.c", "-L", package / "lib",
                            f"-Wl,-rpath,{package / 'lib'}", "-llmdb", "-o", executable])
            if name == "incremental":
                run([executable] + [work / value for value in ("source", "restored", "delta1", "delta2")])
            elif name == "crypto_module":
                run([executable, plugin])
            elif name == "nolock":
                for mode in ("current", "previous"):
                    data = work / ("nolock-" + mode)
                    data.mkdir()
                    run([executable, mode, data])
            elif name == "catalog_capacity":
                for label, flags in (("buffered", 0), ("writemap", 0x80000)):
                    data = work / ("catalog-capacity-" + label)
                    run([executable, "seed", data, flags])
                    run([executable, "cold", data, flags])
            elif name == "catalog_map_capacity":
                for mode in ("catalog", "commit", "prepare"):
                    run([executable, mode, work / ("catalog-map-" + mode)])
            elif name == "jumbo_values":
                for label, flags in (("ordinary", 0), ("cached", 0x4000000), ("writemap", 0x80000)):
                    run([executable, work / ("jumbo-" + label), flags])
            elif name == "maintenance":
                for label, flags in (("buffered", 0), ("cached", 0x4000000), ("writemap", 0x80000)):
                    run([executable, work / ("maintenance-" + label), flags])
                    run([executable, work / ("maintenance-default-" + label), flags, "default"])
            elif name in ("reserve_ordering", "physical_stats", "large_values", "jumbo_preflight"):
                run([executable, work / (name + "-data")])
            else:
                transformed = work / "transformed"
                transformed.mkdir()
                run([executable, transformed])


if __name__ == "__main__":
    main()
