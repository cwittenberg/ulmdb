#!/usr/bin/env python3
"""Reject real missing/no-op prepare/rollback libraries using unchanged apps.

Linux/GNU-linker targeted controls, independent of the full parity profile.
No Cargo build or qualified input mutation is performed. --output must be new.
Applications link only against hash-verified pinned modern LMDB with SONAME0;
the same executables then use package, archive witness and deficient providers.
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
import sys
import time

SYMBOLS = {"prepare": "mdb_txn_prepare", "rollback": "mdb_env_rollback"}
SUITES = {"prepare": ("parity_prepare_suite.c", 84),
          "first_ids": ("parity_first_ids_suite.c", 4),
          "metadata": ("parity_prepare_metadata_suite.c", 32),
          "rollback": ("parity_rollback_suite.c", 55)}


def source_root() -> Path:
    for root in Path(__file__).resolve().parents:
        if (root / "Cargo.toml").is_file() and (root / "clients/lmdb-ulmdb").is_dir():
            return root
    raise RuntimeError("provide --source-root")


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


class Controls:
    def __init__(self, args: argparse.Namespace):
        self.a = args
        self.out = args.output
        self.out.mkdir(parents=True, exist_ok=False)
        self.cc = shlex.split(os.environ.get("CC", "cc"))
        self.receipt: dict = {"schema": 1, "profile": "targeted-lifecycle-controls",
                              "apis": args.apis, "product_qualified": False,
                              "complete": False, "passed": False,
                              "commands": [], "sources": [], "executables": {},
                              "providers": {}, "runs": [], "controls": [],
                              "generated_artifacts": {}}
        self.protected: dict[str, str] = {}
        runner = Path(__file__).resolve()
        frozen_runner = self.out / "runner.py"
        shutil.copyfile(runner, frozen_runner)
        self.protected[str(runner)] = digest(runner)
        require(digest(frozen_runner) == self.protected[str(runner)], "runner changed while copied")
        self.receipt["runner"] = {"source": str(runner), "copy": "runner.py",
                                  "sha256": self.protected[str(runner)]}
        self.artifact(frozen_runner)
        self.exes: dict[str, dict] = {}
        self.reference_traces: dict[str, bytes] = {}
        self.env = os.environ.copy()
        for key in ("LD_PRELOAD", "LD_AUDIT", "LD_DEBUG", "LD_DEBUG_OUTPUT",
                    "LD_LIBRARY_PATH", "LD_TRACE_LOADED_OBJECTS", "ULMDB_LIBRARY",
                    "ULMDB_MIGRATOR", "ULMDB_FAIL_AT", "ULMDB_CRASH_AT"):
            self.env.pop(key, None)
        for key in tuple(self.env):
            if key.startswith("ULMDB_"):
                self.env.pop(key)
        self.env.update(ULMDB_LMDB_ABI="1.0", LD_BIND_NOW="1", LC_ALL="C")

    def save(self) -> None:
        (self.out / "receipt.json").write_text(json.dumps(self.receipt, indent=2) + "\n")

    def artifact(self, path: Path) -> None:
        self.receipt["generated_artifacts"][str(path.relative_to(self.out))] = digest(path)

    def command(self, label: str, argv: list, env: dict | None = None) -> tuple[subprocess.CompletedProcess, dict]:
        command = [str(x) for x in argv]
        started = time.monotonic()
        try:
            result = subprocess.run(command, env=env or self.env,
                                    capture_output=True, timeout=self.a.timeout)
        except subprocess.TimeoutExpired as error:
            (self.out / (label + ".stdout")).write_bytes(error.stdout or b"")
            (self.out / (label + ".stderr")).write_bytes(error.stderr or b"")
            self.receipt["commands"].append({"label": label, "argv": command,
                                              "timeout": True, "passed": False})
            self.save()
            raise RuntimeError(f"{label}: deadline is a harness failure, never an accepted rejection") from error
        stdout = self.out / (label + ".stdout")
        stderr = self.out / (label + ".stderr")
        stdout.write_bytes(result.stdout)
        stderr.write_bytes(result.stderr)
        row = {"label": label, "argv": command, "returncode": result.returncode,
               "seconds": round(time.monotonic() - started, 5),
               "environment": {key: value for key, value in (env or self.env).items()
                               if key.startswith(("ULMDB_", "LD_"))},
               "stdout": str(stdout.relative_to(self.out)), "stderr": str(stderr.relative_to(self.out)),
               "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr)}
        self.receipt["commands"].append(row)
        self.save()
        return result, row

    def successful(self, label: str, argv: list) -> bytes:
        result, _ = self.command(label, argv)
        require(result.returncode == 0, f"{label}: command failed: {result.stderr[-2000:]!r}")
        return result.stdout

    def exports(self, label: str, library: Path) -> set[str]:
        text = self.successful(label, ["nm", "-D", "--defined-only", library]).decode()
        names = {line.split()[-1] for line in text.splitlines() if len(line.split()) >= 3}
        require(all(re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", n) for n in names),
                f"{label}: unsupported versioned/nonidentifier export")
        return names

    def pins(self, source: Path, manifest_name: str) -> dict:
        manifest_path = self.a.source_root / "clients/lmdb-ulmdb" / manifest_name
        pins = json.loads(manifest_path.read_text())
        rows = []
        for entry in pins["files"]:
            path = source / Path(entry["path"]).name
            require(path.is_file() and not path.is_symlink(), f"missing pinned regular source: {path}")
            require(path.stat().st_size == entry["bytes"] and digest(path) == entry["sha256"],
                    f"pinned source changed: {path}")
            self.protected[str(path)] = entry["sha256"]
            rows.append({"path": str(path), "sha256": entry["sha256"]})
        self.protected[str(manifest_path)] = digest(manifest_path)
        return {"ref": pins["ref"], "commit": pins["commit"],
                "manifest_sha256": digest(manifest_path), "files": rows}

    def copy_source(self, source: Path, directory: Path) -> Path:
        require(source.is_file(), f"suite dependency missing: {source}")
        target = directory / source.name
        require(not target.exists(), f"duplicate suite source name: {source.name}")
        shutil.copyfile(source, target)
        self.protected[str(source)] = digest(source)
        require(digest(target) == self.protected[str(source)], f"suite source changed while copied: {source}")
        self.artifact(target)
        self.receipt["sources"].append({"source": str(source), "copy": str(target.relative_to(self.out)),
                                         "sha256": digest(target)})
        return target

    def compile(self, name: str, source: Path, reference: Path, expected_count: int | None) -> None:
        exe = self.out / "apps" / name
        self.successful("compile-" + name, self.cc + ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-pthread", "-I", self.a.lmdb_source, source, "-L", reference.parent,
                        "-llmdb", "-o", exe])
        dynamic = self.successful("dynamic-" + name, ["readelf", "-d", exe]).decode()
        needed = re.findall(r"\(NEEDED\).*?\[([^]]+)\]", dynamic)
        require("liblmdb.so.0" in needed and all("/" not in n for n in needed),
                f"{name}: DT_NEEDED must select SONAME basename, never an input/provider path: {needed}")
        symbols = self.successful("imports-" + name, ["readelf", "--dyn-syms", "--wide", exe]).decode()
        imports = sorted(set(re.findall(r"\bUND\s+(mdb_[A-Za-z0-9_]+)(?:@[^\s]+)?", symbols)))
        row = {"path": str(exe), "sha256": digest(exe), "needed": needed,
               "mdb_imports": imports, "journeys": expected_count}
        self.exes[name] = row
        self.receipt["executables"][name] = row

    def provider_trace(self, provider_name: str, library: Path, name: str) -> dict:
        env = dict(self.env, LD_LIBRARY_PATH=str(library.parent), LD_TRACE_LOADED_OBJECTS="1")
        result, row = self.command("loader-" + provider_name + "-" + name, [self.exes[name]["path"]], env)
        observed = re.findall(rb"liblmdb\.so\.0\s+=>\s+(\S+)", result.stdout)
        require(observed == [str(library).encode()], f"{provider_name}/{name}: wrong loader provider: {observed}")
        require(result.returncode == 0, f"{provider_name}/{name}: loader trace failed")
        return {"command": row, "observed": [x.decode() for x in observed], "library_sha256": digest(library)}

    def expected_rejection(self, api: str, kind: str, name: str, result: subprocess.CompletedProcess) -> str:
        symbol = SYMBOLS[api]
        imports = self.exes[name]["mdb_imports"]
        if kind == "missing" and symbol in imports:
            require(result.returncode == 127 and b"symbol lookup error" in result.stderr
                    and (b"undefined symbol: " + symbol.encode()) in result.stderr,
                    f"{api}/{kind}/{name}: not the required loader rejection")
            return "missing-import loader rejection"
        if kind == "bogus" and api == "prepare" and name in {"prepare", "first_ids"}:
            marker = b"scenario=prepare-clean-fresh-notls0-abort0" if name == "prepare" else b"clean-prepare.writer=1"
            diagnostic = b"next writer:" if name == "prepare" else b"begin="
            require(result.returncode == 1 and marker in result.stdout and diagnostic in result.stderr,
                    f"{api}/{kind}/{name}: not the intended prepare lifecycle rejection")
            return "unconsumed-clean-prepare lifecycle rejection"
        if kind == "bogus" and api == "rollback" and name == "rollback":
            require(result.returncode == 1 and b"case=head2 flags=0" in result.stdout
                    and b"head=2\n" in result.stdout and b"rollback oracle: selected head" in result.stderr,
                    "bogus rollback did not fail the logical head restoration oracle")
            return "unchanged-selected-head rollback rejection"
        require(result.returncode == 0 and result.stdout == self.reference_traces[name],
                f"{api}/{kind}/{name}: unrelated/cross-dependency behavior changed")
        return "unchanged behavior; this suite does not establish the mutated transition"

    def exercise(self, provider_name: str, library: Path, api: str | None = None, kind: str | None = None) -> None:
        provider = self.receipt["providers"][provider_name]
        for name in self.exes:
            evidence = self.provider_trace(provider_name, library, name)
            data = self.out / "data" / provider_name / name
            data.mkdir(parents=True, exist_ok=False)
            env = dict(self.env, LD_LIBRARY_PATH=str(library.parent))
            result, command = self.command("run-" + provider_name + "-" + name,
                                           [self.exes[name]["path"], data], env)
            if api is None:
                require(result.returncode == 0, f"{provider_name}/{name}: positive baseline failed")
                if provider_name == "reference":
                    self.reference_traces[name] = result.stdout
                else:
                    require(result.stdout == self.reference_traces[name], f"{provider_name}/{name}: baseline trace differs")
                outcome = "reference matched"
            else:
                outcome = self.expected_rejection(api, kind, name, result)
            count = self.exes[name]["journeys"]
            if result.returncode == 0 and count is not None:
                marker = b"case=" if name == "rollback" else b"scenario="
                require(result.stdout.count(marker) == count, f"{provider_name}/{name}: incomplete suite")
            require(digest(Path(self.exes[name]["path"])) == self.exes[name]["sha256"], "executable changed")
            self.receipt["runs"].append({"provider": provider_name, "suite": name, "passed": True,
                                         "outcome": outcome, "actual_exit": result.returncode,
                                         "command": command, "provider_evidence": evidence})
            self.save()
        # Inventory is independent of DT_NEEDED suites and deliberately ignores
        # absent modern extensions: all 56 unrelated common functions must exist.
        probe = self.out / "apps/common56"
        result, command = self.command("inventory-" + provider_name, [probe, library])
        require(result.returncode == 0 and result.stdout == b"common56=56\n"
                and result.stderr == ("provider=" + str(library) + "\n").encode(),
                f"{provider_name}: common56/dladdr provider check failed")
        provider["common56"] = {"declared": 56, "exported": 56, "command": command}
        require(digest(library) == provider["sha256"], f"{provider_name}: provider mutated")
        self.save()

    def relink(self, name: str, exports: set[str], symbol: str | None = None, bogus: bool = False) -> Path:
        directory = self.out / "providers" / name
        directory.mkdir(parents=True)
        script = directory / "exports.map"
        script.write_text("{\n  global:\n" + "".join(f"    {s};\n" for s in sorted(exports)) + "  local: *;\n};\n")
        self.artifact(script)
        library = directory / "liblmdb.so.0"
        command = self.cc + ["-shared", "-Wl,-soname,liblmdb.so.0", "-Wl,--version-script=" + str(script)]
        if bogus:
            source = directory / "bogus.c"
            body = ("int mdb_txn_prepare(MDB_txn *txn) {(void)txn;return 0;}\n" if symbol == "mdb_txn_prepare"
                    else "int mdb_env_rollback(MDB_env *env,mdb_size_t id) {(void)env;(void)id;return 0;}\n")
            source.write_text("#include <lmdb.h>\n/* Real public no-op success; no flag/data mutation. */\n" + body)
            obj = directory / "bogus.o"
            self.successful("compile-" + name, self.cc + ["-std=c11", "-O2", "-fPIC", "-Wall", "-Wextra", "-Werror",
                            "-I", self.a.lmdb_source, "-c", source, "-o", obj])
            self.artifact(source)
            self.artifact(obj)
            command += ["-Wl,--allow-multiple-definition", obj]
        command += ["-Wl,--whole-archive", self.out / "inputs/archive.a", "-Wl,--no-whole-archive",
                    "-ldl", "-lpthread", "-lm", "-lrt", "-lutil", "-o", library]
        self.successful("link-" + name, command)
        observed = self.exports("exports-" + name, library)
        require(observed == exports, f"{name}: complete export set changed unexpectedly")
        self.receipt["providers"][name] = {"path": str(library), "sha256": digest(library),
                                            "exports": sorted(observed), "role": name}
        return library

    def perform(self) -> None:
        a = self.a
        for tool in (self.cc, ["ld"], ["readelf"], ["nm"]):
            self.successful("version-" + Path(tool[0]).name, tool + ["--version"])
        selected = ["prepare", "rollback"] if a.apis == "both" else [a.apis]
        modern = self.pins(a.lmdb_source, "modern-source.json")
        legacy = self.pins(a.legacy_source, "legacy-source.json")
        self.receipt["reference_pins"] = {"modern": modern, "legacy_header": legacy}
        library = a.package / "lib/liblmdb.so.0"
        require(library.is_file() and a.archive.is_file(), "package library/static archive missing")
        original_exports = self.exports("exports-input", library)
        require(all(SYMBOLS[x] in original_exports for x in selected), "selected API missing from input; no silent skipping")
        header = re.sub(r"/\*.*?\*/", "", (a.legacy_source / "lmdb.h").read_text(), flags=re.S)
        header = re.sub(r"^\s*#.*$", "", header, flags=re.M)
        common = set()
        for declaration in header.split(";"):
            match = re.search(r"\b(mdb_[A-Za-z0-9_]+)\s*\([^;{}]*\)\s*$", declaration)
            if match:
                common.add(match.group(1))
        require(len(common) == 56 and common <= original_exports, "unrelated common56 inventory is incomplete")
        self.protected[str(library)] = digest(library)
        self.protected[str(a.archive)] = digest(a.archive)
        inputs = self.out / "inputs"
        inputs.mkdir()
        shutil.copyfile(a.archive, inputs / "archive.a")
        require(digest(inputs / "archive.a") == self.protected[str(a.archive)], "archive changed during snapshot")
        self.artifact(inputs / "archive.a")
        package_dir = self.out / "providers/package"
        package_dir.mkdir(parents=True)
        package_library = package_dir / "liblmdb.so.0"
        shutil.copyfile(library, package_library)
        require(digest(package_library) == self.protected[str(library)], "package changed during snapshot")
        self.receipt["providers"]["package"] = {"path": str(package_library), "sha256": digest(package_library),
                                                   "exports": sorted(original_exports), "role": "unaltered package snapshot"}
        reference_dir = self.out / "providers/reference"
        reference_dir.mkdir()
        reference = reference_dir / "liblmdb.so.0"
        self.successful("build-reference", self.cc + ["-std=c11", "-O2", "-D_GNU_SOURCE", "-fPIC", "-shared", "-pthread",
                        "-Wl,-soname,liblmdb.so.0", "-I", a.lmdb_source, a.lmdb_source / "mdb.c",
                        a.lmdb_source / "midl.c", "-o", reference])
        (reference_dir / "liblmdb.so").symlink_to("liblmdb.so.0")
        self.receipt["providers"]["reference"] = {"path": str(reference), "sha256": digest(reference), "role": "pinned reference"}
        sources = self.out / "sources"
        sources.mkdir()
        names = ["prepare", "first_ids", "metadata"] if "prepare" in selected else []
        if "rollback" in selected:
            names.append("rollback")
        dependencies = {"prepare": ["parity_prepare_suite.c", "parity_prepare.c"],
                        "first_ids": ["parity_first_ids_suite.c", "parity_first_ids.c"],
                        "metadata": ["parity_prepare_metadata_suite.c", "parity_prepare_metadata.c"],
                        "rollback": ["parity_rollback_suite.c", "parity_rollback.c"]}
        for name in names:
            for filename in dependencies[name]:
                source = a.rollback_source if filename == "parity_rollback.c" and a.rollback_source else a.suite_root / filename
                self.copy_source(source, sources)
        compatibility = self.copy_source(a.source_root / "clients/c/tests/compatibility.c", sources)
        modern_flags = self.copy_source(a.source_root / "clients/lmdb-ulmdb/tests/parity_modern.c", sources)
        (self.out / "apps").mkdir()
        for name in names:
            self.compile(name, sources / SUITES[name][0], reference, SUITES[name][1])
        self.compile("compatibility", compatibility, reference, None)
        self.compile("modern_flags", modern_flags, reference, 3)
        for name in names:
            required = {"prepare": "mdb_txn_prepare", "first_ids": "mdb_txn_prepare",
                        "metadata": "mdb_txn_prepare", "rollback": "mdb_env_rollback"}[name]
            require(required in self.exes[name]["mdb_imports"], f"{name}: intended import missing")
        if "rollback" in names:
            require("mdb_txn_prepare" in self.exes["rollback"]["mdb_imports"], "rollback leaf's prepare dependency changed")
        require(all(not (set(x["mdb_imports"]) & set(SYMBOLS.values()))
                    for n, x in self.exes.items() if n in {"compatibility", "modern_flags"}), "common smoke imports target API")
        probe = sources / "common56.c"
        names_c = ",\n".join(json.dumps(n) for n in sorted(common))
        probe.write_text('#define _GNU_SOURCE\n#include <dlfcn.h>\n#include <stdio.h>\n'
                         'int main(int argc,char**argv){if(argc!=2)return 2;void*h=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL|RTLD_NODELETE);if(!h)return 1;'
                         'const char*n[]={' + names_c + '};void*first=0;for(unsigned i=0;i<56;i++){void*p=dlsym(h,n[i]);if(!p)return 1;if(!i)first=p;}'
                         'Dl_info d;if(!dladdr(first,&d)||!d.dli_fname)return 1;fprintf(stderr,"provider=%s\\n",d.dli_fname);puts("common56=56");return 0;}\n')
        self.successful("compile-common56", self.cc + ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", probe, "-ldl", "-o", self.out / "apps/common56"])
        self.artifact(probe)
        self.artifact(self.out / "apps/common56")
        self.receipt["common56"] = {"names": sorted(common), "probe_sha256": digest(probe),
                                     "executable_sha256": digest(self.out / "apps/common56")}
        self.exercise("reference", reference)
        (self.out / "reference-frozen.json").write_text(json.dumps({"executables": self.exes,
              "traces": {n: hashlib.sha256(t).hexdigest() for n, t in self.reference_traces.items()},
              "reference_library_sha256": digest(reference), "complete": True, "passed": True}, indent=2) + "\n")
        self.exercise("package", package_library)
        witness = self.relink("archive-witness", original_exports)
        self.exercise("archive-witness", witness)
        for api in selected:
            for kind in ("missing", "bogus"):
                name = api + "-" + kind
                symbols = original_exports - {SYMBOLS[api]} if kind == "missing" else original_exports
                mutant = self.relink(name, symbols, SYMBOLS[api], kind == "bogus")
                self.exercise(name, mutant, api, kind)
                self.receipt["controls"].append({"api": api, "kind": kind, "provider": name,
                    "passed": True, "target": SYMBOLS[api], "exports_delta": [SYMBOLS[api]] if kind == "missing" else [],
                    "required_rejections_observed": [x["suite"] for x in self.receipt["runs"]
                        if x["provider"] == name and x["actual_exit"] != 0]})
                self.save()
        require(all(digest(Path(p)) == h for p, h in self.protected.items()), "qualified/source inputs changed during controls")
        require(all(digest(self.out / p) == h for p, h in self.receipt["generated_artifacts"].items()),
                "generated source/object/archive changed during controls")
        self.receipt.update(complete=True, passed=True, protected_inputs=self.protected,
                            limits=["Targeted controls, not whole-product qualification or performance acceptance",
                                    "Metadata and rollback can pass a no-op prepare: only transition-specific oracles establish preparation",
                                    "Explicit ABI1.0 selection; no automatic loader/profile selection claim"])
        self.save()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apis", choices=("prepare", "rollback", "both"), default="prepare")
    parser.add_argument("--package", type=Path)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-root", type=Path)
    parser.add_argument("--suite-root", type=Path, default=None)
    parser.add_argument("--lmdb-source", type=Path, default=None)
    parser.add_argument("--legacy-source", type=Path, default=Path.home() / ".cache/ulmdb-compat/lmdb-0.9.31")
    parser.add_argument("--rollback-source", type=Path)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    require(sys.platform.startswith("linux"), "this control runner requires Linux/GNU ELF tools")
    args.source_root = (args.source_root or source_root()).resolve()
    args.package = args.package or args.source_root / "target/lmdb-ulmdb"
    args.archive = args.archive or args.source_root / "target/debug/libulmdb_c.a"
    args.suite_root = (args.suite_root or args.source_root / "clients/lmdb-ulmdb/tests").resolve()
    args.lmdb_source = (args.lmdb_source or args.source_root / "benchmark/support/lmdb/vendor").resolve()
    for name in ("package", "archive", "output", "legacy_source"):
        setattr(args, name, getattr(args, name).resolve())
    if args.rollback_source:
        args.rollback_source = args.rollback_source.resolve()
    require(args.timeout > 0, "timeout must be positive")
    controls = Controls(args)
    try:
        controls.perform()
    except BaseException as error:
        controls.receipt.update(complete=False, passed=False, error=repr(error), protected_inputs=controls.protected)
        controls.save()
        raise
    print(f"PASS: {args.apis} lifecycle controls; receipt: {args.output / 'receipt.json'}")


if __name__ == "__main__":
    main()
