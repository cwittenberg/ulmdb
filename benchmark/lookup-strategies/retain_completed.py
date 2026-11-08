#!/usr/bin/env python3
"""Retain completed lookup evidence after every timing process has stopped.

Selection is deliberate: frozen source manifests, flat metadata and approved
raw/tooling subdirectories only. Database fixtures and build trees are never
walked. Large executable bytes are verified against receipts, then omitted.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
CAMPAIGNS = ("direct-optimized2", "direct-primary", "regression", "confirmation", "regression-canonical")
SOURCE_DIRECTORIES = ("baseline", "candidate", "candidate-direct", "candidate-final", "candidate-optimized",
                      "candidate-optimized2", "allocation-qualified", "live-integration")
SKIP_DIRS = {"target", "databases", "fixtures", "canonical-build", "example-db", "__pycache__", ".git"}
TEXT_SUFFIXES = {".json", ".md", ".csv", ".log", ".txt", ".py", ".toml", ".lock"}
ALLOWED_EXISTING = {"crates/ulmdb/src/lib.rs", "crates/ulmdb/src/resident/mod.rs",
                    "crates/ulmdb-storage/src/resident/directory.rs"}
PUBLIC_REGRESSION_RECEIPTS = {"table.json", "comparison.json", "provenance.json", "CODEGEN_REVIEW.json",
                              "ADMISSION_REVIEW.json", "PRIMARY_VS_CONFIRMATION.json", "qualification.json",
                              "table.csv", "comparison.csv"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    require(path.is_file() and not path.is_symlink(), f"expected regular evidence file: {path}")
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def load(path):
    require(path.is_file() and not path.is_symlink(), f"missing regular JSON evidence: {path}")
    return json.loads(path.read_text())


def safe_relative(root, name):
    path = PurePosixPath(name)
    require(not path.is_absolute() and ".." not in path.parts, f"unsafe receipt path: {name}")
    result = root.joinpath(*path.parts)
    require(result.resolve().is_relative_to(root.resolve()), f"receipt path escaped root: {name}")
    require(not result.is_symlink(), f"symlink evidence rejected: {result}")
    return result


def source_manifest(value):
    if isinstance(value, dict) and "sha256" in value and isinstance(value["sha256"], dict):
        value = value["sha256"]
    require(isinstance(value, dict) and value and all(isinstance(k, str) and isinstance(v, str)
            and len(v) == 64 for k, v in value.items()), "invalid frozen source manifest")
    return value


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


class Verification:
    def __init__(self, work):
        self.work = work
        self.checked = {}
        self.sources = {}
        self.binaries = {}
        self.campaigns = {}
        self.diagnostics = {}
        self.source_relocations = []

    def checked_file(self, path, expected):
        path = path.resolve()
        require(path.is_relative_to(self.work), f"receipt references data outside work tree: {path}")
        actual = digest(path)
        require(actual == expected, f"SHA-256 mismatch: {path}")
        old = self.checked.get(path)
        require(old is None or old == actual, f"conflicting evidence hashes: {path}")
        self.checked[path] = actual
        return actual

    def source(self, path, hashes):
        path = path.resolve()
        require(path.is_relative_to(self.work) and path.name == "source", "unrecognized frozen source location")
        hashes = source_manifest(hashes)
        previous = self.sources.get(path)
        require(previous is None or previous == hashes, f"conflicting source manifests: {path}")
        if previous is None:
            for name, expected in hashes.items():
                self.checked_file(safe_relative(path, name), expected)
            self.sources[path] = hashes

    def binary(self, path, expected):
        path = Path(path).resolve()
        self.checked_file(path, expected)
        self.binaries[str(path.relative_to(self.work))] = expected

    def diagnostic_direct(self, name):
        """Earlier failed protocols are preserved separately, never admitted."""
        directory = self.work / name
        if not (directory / "results.json").is_file():
            return
        document = load(directory / "results.json")
        if document.get("source") and document.get("source_hashes"):
            original = Path(document["source"]).resolve()
            hashes = source_manifest(document["source_hashes"])
            candidates = [original] + [self.work / n / "source" for n in SOURCE_DIRECTORIES]
            located = None
            for candidate in candidates:
                manifest = candidate.parent / "source-sha256.json"
                if candidate in self.sources and self.sources[candidate] == hashes:
                    located = candidate
                    break
                if manifest.is_file() and source_manifest(load(manifest)) == hashes:
                    located = candidate
                    break
            require(located is not None, f"failed protocol source manifest has no retained exact snapshot: {name}")
            self.source(located, hashes)
            if located != original:
                self.source_relocations.append({"campaign": name, "recorded_directory": str(original),
                                                "verified_directory": str(located),
                                                "basis": "exact complete receipt source manifest equality; original receipt retained unchanged"})
        if document.get("binary") and document.get("binary_sha256"):
            self.binary(document["binary"], document["binary_sha256"])
        for run in document.get("runs", []):
            self.checked_file(safe_relative(directory, run["raw_file"]), run["raw_sha256"])
        self.diagnostics[name] = {"classification": "prior failed/build-only protocol; excluded from final admitted timing set",
                                  "complete": document.get("complete", False), "accepted_before_failure": len(document.get("runs", [])),
                                  "status": document.get("status"), "failures": document.get("failures", [])}

    def direct(self, name, helper):
        directory = self.work / name
        document = load(directory / "results.json")
        require(document.get("complete") is True, f"campaign is incomplete: {name}")
        require(document.get("benchmark") == "production_lookup_direct_campaign", f"wrong direct campaign: {name}")
        self.source(Path(document["source"]), document["source_hashes"])
        self.binary(document["binary"], document["binary_sha256"])
        config = document["configuration"]
        expected_jobs = helper.schedule(config["sizes"], config["rounds"], config["seed"])
        runs = document["runs"]
        require(len(runs) == len(expected_jobs) and {r["id"] for r in runs} == {j["id"] for j in expected_jobs},
                f"direct process inventory incomplete: {name}")
        for run in runs:
            raw = safe_relative(directory, run["raw_file"])
            self.checked_file(raw, run["raw_sha256"])
            require(load(raw) == run["report"], f"raw/parsed direct report differs: {raw}")
            helper.validate_report(run["report"], run, config)
        helper.validate_cross_arm(runs)
        require(helper.tree_hash(document["source_hashes"]) == document["source_manifest_sha256"],
                f"source manifest digest differs: {name}")
        self.checked_file(directory / "direct_campaign.py", document["runner_sha256"])
        self.checked_file(directory / "build.log", document["build_log_sha256"])
        for required in ("FULL_TABLE.md", "FULL_TABLE.csv", "summary.json"):
            require((directory / required).is_file(), f"completed direct artifact missing: {name}/{required}")
        self.campaigns[name] = {"complete": True, "processes": len(runs),
                                "raw_checksums_and_operation_inventory_verified": True,
                                "source_and_executable_verified": True,
                                "failures_retained": len(document.get("failures", []))}
        return document

    def regression(self, name):
        directory = self.work / name
        document = load(directory / "results.json")
        require(document.get("complete") is True, f"campaign is incomplete: {name}")
        for arm, source in document["sources"].items():
            self.source(Path(source["directory"]), source["sha256"])
            binary = document["binaries"][arm]
            self.binary(binary["path"], binary["sha256"])
        runs = document["runs"]
        require(runs, f"no regression processes: {name}")
        require(len({(r["pair"], r["filesystem"], r["arm"]) for r in runs}) == len(runs),
                f"duplicate regression process: {name}")
        expected = {(j["pair"], j["filesystem"], j["arm"]) for j in document["schedule"]}
        require({(r["pair"], r["filesystem"], r["arm"]) for r in runs} == expected,
                f"incomplete scheduled regression inventory: {name}")
        for run in runs:
            raw = safe_relative(directory, run["raw_file"])
            latency = safe_relative(directory, run["latency_file"])
            self.checked_file(raw, run["raw_sha256"])
            self.checked_file(latency, run["latency_sha256"])
            report = load(raw)
            require(report.get("semantic_validation_passed") is True and report.get("timing_enabled") is True,
                    f"regression semantic validation failed: {raw}")
            require(report.get("native_flags") == 0, f"regression LMDB durability relaxed: {raw}")
            require(report.get("binary_sha256") == document["binaries"][run["arm"]]["sha256"],
                    f"per-process executable identity differs: {raw}")
            require(report.get("suite") == "full15" and len(report["reads"]) == 13 and len(report["writes"]) == 2,
                    f"full engine case inventory missing: {raw}")
            manifest = document["sources"][run["arm"]]["sha256"]
            require(report.get("source_hashes") and all(manifest.get(p) == value for p, value in report["source_hashes"].items()),
                    f"per-process source identity differs: {raw}")
        for key, subdir in (("tooling_sha256", "tooling"), ("execution_tooling_sha256", "tooling-execution")):
            for path, expected_hash in document.get(key, {}).items():
                self.checked_file(safe_relative(directory / subdir, path), expected_hash)
        if "primary_artifacts_sha256" in document:
            primary = Path(document["primary_directory"]).resolve()
            require(primary.is_relative_to(self.work), "confirmation primary escaped work tree")
            for path, expected_hash in document["primary_artifacts_sha256"].items():
                self.checked_file(safe_relative(primary, path), expected_hash)
        for required in ("REGRESSION.md", "COMPARISON.md", "table.json", "comparison.json", "baseline.full.json", "candidate.full.json"):
            require((directory / required).is_file(), f"completed regression artifact missing: {name}/{required}")
        self.campaigns[name] = {"complete": True, "processes": len(runs),
                                "source_executable_raw_and_latency_hashes_verified": True,
                                "normal_durability_and_full_case_inventory_verified": True}
        return document


def owned_delta(baseline, candidate):
    def runtime(path):
        return path.startswith(("crates/", "clients/"))

    def owned(path):
        return path in ALLOWED_EXISTING or path == "crates/ulmdb/src/lookup.rs" or path.startswith("crates/ulmdb/src/lookup/") or path.startswith("crates/ulmdb/tests/lookup_strategy") or path == "crates/ulmdb/examples/lookup_strategy.rs"

    changes = [p for p in sorted(set(baseline) | set(candidate)) if runtime(p) and baseline.get(p) != candidate.get(p)]
    require(changes and all(owned(path) for path in changes), "unrelated production source changes in final candidate")
    require(not any(path not in candidate for path in changes), "final candidate deleted production source")
    return {"owned_only": True, "allowed_existing_files": sorted(ALLOWED_EXISTING), "actual_runtime_delta": changes,
            "baseline_sha256": {p: baseline.get(p) for p in changes},
            "candidate_sha256": {p: candidate.get(p) for p in changes}}


def select_files(work, verification):
    """Never descend unknown directories that may contain multi-GB fixtures."""
    files = {}

    def add(path):
        require(path.is_file() and not path.is_symlink(), f"nonregular evidence selected: {path}")
        files["work/" + path.relative_to(work).as_posix()] = path

    def flat(directory, all_files=False):
        if directory.is_dir():
            for path in directory.iterdir():
                if path.is_file() and (all_files or path.suffix in TEXT_SUFFIXES):
                    add(path)

    def approved_tree(directory):
        if not directory.is_dir():
            return
        for root, directories, names in os.walk(directory, followlinks=False):
            directories[:] = [d for d in directories if d not in SKIP_DIRS and not d.endswith("-target")
                              and not (Path(root) / d).is_symlink()]
            for name in names:
                path = Path(root) / name
                if path.suffix in TEXT_SUFFIXES or path.name.endswith(".latencies.u64le"):
                    add(path)

    flat(work)
    for directory in SOURCE_DIRECTORIES:
        base = work / directory
        flat(base)
        manifest = base / "source-sha256.json"
        if manifest.is_file():
            verification.source(base / "source", source_manifest(load(manifest)))
        if directory == "allocation-qualified" and (base / "manifest.json").exists():
            record = load(base / "manifest.json")
            for field in ("runtime_files_sha256", "probe_files_sha256"):
                for name, expected_hash in record.get(field, {}).items():
                    verification.checked_file(safe_relative(base / "source", name), expected_hash)
                    add(safe_relative(base / "source", name))
            for gate in base.iterdir():
                if gate.is_dir() and gate.name.startswith("gate-"):
                    flat(gate, all_files=True)  # Explicit child stdout/stderr/result only; database subdirectory is not entered.
    for source, manifest in verification.sources.items():
        for name in manifest:
            add(safe_relative(source, name))
    omitted_binaries = {(work / relative).resolve() for relative in verification.binaries}
    for path in verification.checked:
        if path not in omitted_binaries and not any(part in SKIP_DIRS or part.endswith("-target") for part in path.relative_to(work).parts):
            add(path)  # Include every verified raw/latency receipt even if its extension changes.
    for name in CAMPAIGNS + ("direct", "direct-optimized"):
        base = work / name
        flat(base)
        for directory in ("raw", "tooling", "tooling-execution"):
            approved_tree(base / directory)
    for base in work.iterdir():
        if base.is_dir() and (base.name.startswith("preflight") or base.name.startswith("prefix-preflight")):
            flat(base)  # Nested fixed16/hostname/n1000000-* directories are database fixtures.
    flat(work / "final-tests")
    for name in ("raw", "logs", "reports", "tooling", "allocation"):
        approved_tree(work / "final-tests" / name)
    for path in HERE.iterdir():
        if path.is_file() and path.suffix in {".py", ".md"}:
            files["review-tooling/" + path.name] = path
    for name in ("compare.py", "report.py", "run.py", "run.sh"):
        path = HERE.parent / name
        if path.is_file():
            files["review-tooling/benchmark/" + name] = path
    return files


def copy_artifacts(work, destination):
    copied = {}

    def copy(source, relative):
        require(source.is_file() and not source.is_symlink(), f"missing public evidence artifact: {source}")
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        copied[relative] = {"source": str(source.relative_to(work)), "bytes": target.stat().st_size,
                            "sha256": digest(target)}

    for name in ("FULL_TABLE.md", "FULL_TABLE.csv", "results.json", "summary.json"):
        copy(work / "direct-optimized2" / name, name)
        copy(work / "direct-primary" / name, "prior-direct/" + name)
    copy(work / "direct-optimized2/FULL_TABLE.md", "LOOKUP_FULL_TABLE.md")
    copy(work / "direct-optimized2/FULL_TABLE.csv", "LOOKUP_FULL_TABLE.csv")
    for directory in ("regression", "confirmation", "regression-canonical"):
        for source in (work / directory).iterdir():
            # Embedded full/partial/results captures remain complete in the
            # archive; public copies contain reports and explicit small receipts.
            if source.is_file() and (source.suffix == ".md" or source.name in PUBLIC_REGRESSION_RECEIPTS):
                copy(source, directory + "/" + source.name)
    for suffix in ("md", "json"):
        copy(work / "confirmation" / ("PRIMARY_VS_CONFIRMATION." + suffix), "REPEATED_REGRESSION." + suffix)
    for source, target in (("regression/REGRESSION.md", "DEFAULT_PRIMARY.md"),
                           ("confirmation/REGRESSION.md", "DEFAULT_CONFIRMATION.md"),
                           ("confirmation/PRIMARY_VS_CONFIRMATION.md", "DEFAULT_REPEATS.md"),
                           ("regression-canonical/REGRESSION.md", "DEFAULT_CANONICAL.md")):
        copy(work / source, target)
    return copied


class HashingReader:
    def __init__(self, stream):
        self.stream = stream
        self.hash = hashlib.sha256()

    def read(self, size=-1):
        data = self.stream.read(size)
        self.hash.update(data)
        return data


def archive_payload(destination, selected, expected):
    records = {}
    with tarfile.open(destination, "w:xz", preset=0) as archive:
        for name, path in sorted(selected.items()):
            before = path.stat()
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = before.st_size, 0o644, 0
            with path.open("rb") as stream:
                reader = HashingReader(stream)
                archive.addfile(info, reader)
            after = path.stat()
            require((before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns),
                    f"evidence changed while archiving: {path}")
            actual = reader.hash.hexdigest()
            if path.resolve() in expected:
                require(actual == expected[path.resolve()], f"verified source/raw changed before archiving: {path}")
            records[name] = {"bytes": info.size, "sha256": actual}
        manifest = json.dumps({"schema": 1, "files": records,
                               "scope": "every payload file except this self-describing manifest; outer MANIFEST.json also hashes this manifest"},
                              sort_keys=True, indent=2).encode() + b"\n"
        info = tarfile.TarInfo("EVIDENCE_MANIFEST.json")
        info.size, info.mode, info.mtime = len(manifest), 0o644, 0
        archive.addfile(info, io.BytesIO(manifest))
        records[info.name] = {"bytes": len(manifest), "sha256": hashlib.sha256(manifest).hexdigest()}
    return records


def output_files(output):
    result = {}
    for root, directories, names in os.walk(output, followlinks=False):
        require(not any((Path(root) / d).is_symlink() for d in directories), "symlink directory in retained output")
        for name in names:
            path = Path(root) / name
            relative = path.relative_to(output).as_posix()
            if relative in {"MANIFEST.json", "SHA256SUMS"}:
                continue
            result[relative] = {"bytes": path.stat().st_size, "sha256": digest(path)}
    return result


def seal_output(output):
    """Include subsequently written REPORT/README files without changing evidence."""
    manifest = load(output / "MANIFEST.json")
    archive = safe_relative(output, manifest["archive"]["file"])
    require(digest(archive) == manifest["archive"]["sha256"], "archive changed; cannot reseal new evidence implicitly")
    manifest["output_files"] = output_files(output)
    manifest["output_files_scope"] = "every regular output file at the last seal, except MANIFEST.json and SHA256SUMS self-references; later additions require --seal-output"
    manifest["sealed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    write_json(output / "MANIFEST.json", manifest)
    sums = [(row["sha256"], path) for path, row in manifest["output_files"].items()]
    sums.append((digest(output / "MANIFEST.json"), "MANIFEST.json"))
    (output / "SHA256SUMS").write_text("".join(f"{value}  {name}\n" for value, name in sorted(sums, key=lambda r: r[1])))


def verify_output(output):
    manifest = load(output / "MANIFEST.json")
    require(output_files(output) == manifest["output_files"], "retained output differs from last seal; verify evidence or reseal added reports")
    archive = safe_relative(output, manifest["archive"]["file"])
    require(digest(archive) == manifest["archive"]["sha256"], "compressed evidence archive hash differs")
    seen = set()
    with tarfile.open(archive, "r:*") as stream:
        for member in stream:
            require(member.isfile() and member.name not in seen and member.name in manifest["archived_files"],
                    "unexpected or duplicate archive member")
            expected = manifest["archived_files"][member.name]
            require(member.size == expected["bytes"], f"archive size differs: {member.name}")
            h = hashlib.sha256()
            payload = stream.extractfile(member)
            require(payload is not None, "archive member cannot be read")
            for block in iter(lambda: payload.read(1 << 20), b""):
                h.update(block)
            require(h.hexdigest() == expected["sha256"], f"archived payload hash differs: {member.name}")
            seen.add(member.name)
    require(seen == set(manifest["archived_files"]), "archive payload inventory differs")
    expected_sums = {name: value["sha256"] for name, value in manifest["output_files"].items()}
    expected_sums["MANIFEST.json"] = digest(output / "MANIFEST.json")
    actual_sums = {}
    for line in (output / "SHA256SUMS").read_text().splitlines():
        value, name = line.split("  ", 1)
        require(name not in actual_sums, "duplicate outer checksum entry")
        actual_sums[name] = value
    require(actual_sums == expected_sums, "outer SHA256SUMS differs")
    print(json.dumps({"verified_output": str(output), "archived_files": len(seen), "output_files": len(expected_sums)}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--replace", action="store_true", help="move existing output to a sibling backup before installing new evidence")
    parser.add_argument("--seal-output", action="store_true", help="reseal all output files after adding final REPORT/README; archive must remain unchanged")
    parser.add_argument("--verify-only", action="store_true", help="verify the retained archive, every archived payload and sealed output without original work/targets")
    args = parser.parse_args()
    require(not args.output.is_symlink(), "symlink output rejected")
    if args.seal_output or args.verify_only:
        if args.seal_output:
            seal_output(args.output.resolve())
        if args.verify_only:
            verify_output(args.output.resolve())
        return
    require(args.work is not None, "--work is required when retaining a new evidence package")
    work, output = args.work.resolve(), args.output.resolve()
    require(work.is_dir() and work != output and output not in work.parents,
            "output must not replace or contain the input work tree")
    require(not output.exists() or args.replace, "output exists; choose a fresh path or explicitly use --replace")
    require(not output.is_symlink(), "symlink output rejected")
    sys.path.insert(0, str(HERE))
    import direct_campaign as direct
    verification = Verification(work)
    latest = verification.direct("direct-optimized2", direct)
    verification.direct("direct-primary", direct)
    for name in ("regression", "confirmation", "regression-canonical"):
        verification.regression(name)
    for name in ("direct", "direct-optimized"):
        verification.diagnostic_direct(name)
    baseline = source_manifest(load(work / "baseline/source-sha256.json"))
    final = source_manifest(latest["source_hashes"])
    delta = owned_delta(baseline, final)
    require(load(work / "candidate/ownership-delta.json").get("unrelated_engine_changes") is False,
            "original ownership qualification does not establish an isolated feature delta")
    selected = select_files(work, verification)
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix="." + output.name + "-retain-", dir=output.parent))
    try:
        copied = copy_artifacts(work, staging)
        qualification = {"schema": 1, "status": "no_universal_nonregression_claim",
                         "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                         "scope": "completed production bytewise snapshot-scoped lookup capability; synthetic warm direct comparison and frozen default-path regression evidence",
                         "universal_nonregression_proven": False, "adverse_evidence_preserved": True,
                         "source_delta_owned_only": True, "owned_source_delta": delta,
                         "source_delta_basis": "original frozen baseline versus the final optimized2 candidate; separate live-integration sources are additional explicitly labeled evidence",
                         "campaigns": verification.campaigns,
                         "prior_protocol_diagnostics": verification.diagnostics,
                         "diagnostic_source_relocations": verification.source_relocations,
                         "binary_hashes_verified_but_executable_bytes_not_archived": verification.binaries,
                         "source_files_verified": {str(path.relative_to(work)): len(manifest) for path, manifest in verification.sources.items()},
                         "verified_source_raw_latency_and_binary_files": len(verification.checked),
                         "archive_selection": "frozen manifest files, flat reports/logs, raw traces, captured tooling, final tests and allocation evidence; targets/databases/fixtures/canonical-build excluded without traversal",
                         "allocation_evidence": {"manifest": load(work / "allocation-qualified/manifest.json") if (work / "allocation-qualified/manifest.json").exists() else None,
                                                 "child_gate_results": {p.parent.name: load(p) for p in (work / "allocation-qualified").glob("gate-*/result.json")}},
                         "proof_limits": "hashes establish retained byte identity, not statistical equivalence, controlled pressure or universal performance; earlier failed protocols and adverse captures remain in the archive"}
        write_json(staging / "qualification.json", qualification)
        selected["qualification.json"] = staging / "qualification.json"
        archive = staging / "evidence.tar.xz"
        records = archive_payload(archive, selected, verification.checked)
        # Verify the staged public copies against their source before accepting.
        for relative, row in copied.items():
            require(digest(work / row["source"]) == row["sha256"], f"public evidence source changed: {relative}")
        manifest = {"schema": 1, "archive": {"file": archive.name, "bytes": archive.stat().st_size,
                                              "sha256": digest(archive)},
                    "archived_files": records, "public_copies": copied,
                    "qualification_sha256": digest(staging / "qualification.json"),
                    "omitted": "executable bytes (receipt SHA-256 verified), target/build trees, all database fixtures and canonical-build temporary tree",
                    "self_reference": "this outer manifest is hashed in SHA256SUMS; archive includes an independently hashed inner payload manifest"}
        write_json(staging / "MANIFEST.json", manifest)
        seal_output(staging)
        if output.exists():
            stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
            output.rename(output.with_name(output.name + ".previous-" + stamp))
        staging.rename(output)
        print(json.dumps({"output": str(output), "archive_sha256": manifest["archive"]["sha256"],
                          "archived_files": len(records), "campaigns": verification.campaigns}, indent=2))
    except Exception:
        # Preserve incomplete staging for diagnosis; never replace accepted data.
        print(f"Retention did not publish; incomplete staging preserved at {staging}", file=sys.stderr)
        raise


if __name__ == "__main__":
    main()
