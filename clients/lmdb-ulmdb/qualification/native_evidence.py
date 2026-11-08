#!/usr/bin/env python3
"""Typed native coordination evidence; never an LMDB differential oracle."""
from __future__ import annotations

import hashlib
import json
import re
import subprocess
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[3]
POLICY = Path(__file__).with_name("native_cases.json")
PROFILE = "ulmdb-file-coordination-v1"
KIND = "native-test"
HASH = re.compile(r"[0-9a-f]{64}\Z")
MARKER = re.compile(r"CASE_PASS test=([A-Za-z0-9_:]+) backend=([a-z0-9-]+)")
SOURCE_ROOTS = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates", "clients/c",
                "clients/lmdb-ulmdb", "benchmark/support/lmdb/vendor"]
EXTRA_INPUTS = {"clients/lmdb-ulmdb/qualification/native_cases.json",
                "clients/lmdb-ulmdb/qualification/native_evidence.py",
                "clients/lmdb-ulmdb/qualification/run_native.py"}


class Invalid(ValueError):
    pass


def need(condition, message):
    if not condition:
        raise Invalid(message)


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def text_digest(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def aggregate(files):
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def source_snapshot(root=ROOT):
    result = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", *SOURCE_ROOTS],
                            cwd=root, check=True, capture_output=True)
    paths = set(result.stdout.decode().strip("\0").split("\0")) | EXTRA_INPUTS
    selected = []
    for name in sorted(paths):
        path = Path(name)
        if not name:
            continue
        if name.startswith(("clients/lmdb-ulmdb/conformance/", "clients/lmdb-ulmdb/features/")):
            continue
        if name.startswith("clients/lmdb-ulmdb/qualification/") and name not in EXTRA_INPUTS:
            continue
        if path.suffix not in {".rs", ".c", ".h", ".toml", ".lock", ".map", ".json", ".py"} and path.name != "Makefile":
            continue
        actual = root / name
        need(actual.is_file() and not actual.is_symlink(), f"source input missing/nonregular: {name}")
        selected.append({"path": name, "bytes": actual.stat().st_size, "sha256": digest(actual)})
    need(selected, "empty native source manifest")
    return {"algorithm": "sha256", "files": selected, "sha256": aggregate(selected)}


def load_policy(root=ROOT):
    policy = json.loads((root / POLICY.relative_to(ROOT)).read_text())
    need(policy.get("schema") == 1 and policy.get("kind") == KIND and policy.get("profile") == PROFILE,
         "invalid native case policy")
    need(policy.get("allowed_features") == ["G05"], "native profile must remain scoped to G05")
    seen = set()
    for suite in policy["suites"]:
        need(suite["id"] not in seen, "duplicate native suite")
        seen.add(suite["id"])
        cases = set()
        for case in suite["cases"]:
            need(case["test"] not in cases and not case["test"].endswith("_worker"), "invalid/duplicate native case")
            cases.add(case["test"])
            need(case["backends"] and len(set(case["backends"])) == len(case["backends"]), "invalid backend scope")
            need(case["roles"] and set(case["roles"]) <= {"positive", "negative", "reopen", "resource", "prerequisite"},
                 "invalid native case role")
            if not case["markers"]:
                need(case["roles"] == ["prerequisite"], "unmarked tests cannot qualify native feature backends")
    return policy


def capture_workspace(value):
    """Parse historical provenance without accessing the captured filesystem."""
    need(isinstance(value, str) and value and "\0" not in value, "invalid captured workspace root")
    path = PurePosixPath(value)
    need(path.is_absolute() and not value.startswith("//") and ".." not in path.parts
         and str(path) == value, "invalid captured workspace root")
    return path


def relative_artifact(value):
    need(isinstance(value, str) and value and "\0" not in value, "invalid artifact path")
    path = PurePosixPath(value)
    need(not path.is_absolute() and ".." not in path.parts and str(path) == value
         and len(path.parts) > 1 and path.parts[0] == "target", "invalid artifact path")
    return path


def artifact_path(value, root):
    relative_artifact(value)
    path = (root / value).resolve()
    need(path.is_relative_to((root / "target").resolve()), "native artifact must be inside repository target/")
    return path


def check_log(log, root, required):
    need(isinstance(log.get("text"), str), "native log text missing")
    contents = log["text"].encode("utf-8")
    need(len(contents) == log.get("bytes") and text_digest(log["text"]) == log.get("sha256"), "native log hash mismatch")
    path = artifact_path(log["path"], root)
    if required or path.exists():
        need(path.is_file() and digest(path) == log["sha256"], f"native log artifact mismatch: {log['path']}")
    return log["text"]


def check_step(step, root, required):
    need(step.get("exit_code") == 0 and step.get("passed") is True, "native command failed")
    need(isinstance(step.get("command"), list) and step["command"] and all(isinstance(x, str) for x in step["command"]),
         "native command argv missing")
    stdout = check_log(step["stdout"], root, required)
    check_log(step["stderr"], root, required)
    return stdout


def build_command(spec):
    command = ["cargo", "test", "--locked", "--no-run", "--message-format=json", "-p", spec["package"]]
    command += ["--lib"] if spec["target"] == "lib" else ["--test", spec["target"]]
    if spec["features"]:
        command += ["--features", ",".join(spec["features"])]
    return command


def check_executable(executable, root, required):
    need(HASH.fullmatch(executable.get("sha256", "")) and executable.get("bytes", 0) > 0,
         "native executable hash missing")
    path = artifact_path(executable["path"], root)
    if required or path.exists():
        need(path.is_file() and path.stat().st_size == executable["bytes"] and digest(path) == executable["sha256"],
             "frozen native executable mismatch")
    return path


def check_native(receipt, label="native", root=ROOT, require_artifacts=False):
    need(receipt.get("schema") == 1 and receipt.get("kind") == KIND and receipt.get("profile") == PROFILE,
         f"{label}: invalid native receipt kind/profile")
    need(receipt.get("complete") is True and receipt.get("passed") is True, f"{label}: native run failed/incomplete")
    need(receipt.get("platform", {}).get("system") == "Linux", f"{label}: unsupported native platform")
    captured_root = capture_workspace(receipt.get("capture_root"))
    need(receipt.get("oracle") == "native assertions and observed process/filesystem state",
         f"{label}: incorrect native oracle label")
    need(not ({"adapter", "common_symbols", "modern_additional_symbols"} & set(receipt)),
         f"{label}: native evidence cannot impersonate C exports")
    snapshot = receipt.get("source", {})
    need(snapshot.get("algorithm") == "sha256" and snapshot.get("sha256") == aggregate(snapshot.get("files", [])),
         f"{label}: native source manifest hash mismatch")
    need(snapshot == source_snapshot(root), f"{label}: native source hashes differ from current inputs")
    policy = load_policy(root)
    need(receipt.get("policy_sha256") == digest(root / POLICY.relative_to(ROOT)), f"{label}: native policy hash mismatch")
    adapter = receipt.get("adapter_input", {})
    check_executable(adapter, root, require_artifacts)
    captured_adapter = captured_root / relative_artifact(adapter["path"])
    need(receipt.get("adapter_policy") == "process-lifetime RTLD_NODELETE; no dynamic-unload qualification",
         f"{label}: missing mixed-adapter lifetime scope")
    suites = receipt.get("suites", [])
    expected_suites = {suite["id"]: suite for suite in policy["suites"]}
    need(len(suites) == len(expected_suites) and {suite.get("id") for suite in suites} == set(expected_suites),
         f"{label}: missing/duplicate native suite")
    results = receipt.get("results", [])
    expected_results = set()
    by_suite = {}
    for suite in suites:
        spec = expected_suites[suite["id"]]
        need(suite.get("source") == spec["source"] and suite.get("source_sha256") == digest(root / spec["source"]),
             f"{label}: wrong native test source")
        executable = suite["executable"]
        check_executable(executable, root, require_artifacts)
        captured_executable = captured_root / relative_artifact(executable["path"])
        command = build_command(spec)
        need(suite["build"]["command"] == command, f"{label}: unexpected native build command")
        check_step(suite["build"], root, require_artifacts)
        need(suite["list"]["command"] == [str(captured_executable), "--list", "--format", "terse"], f"{label}: unexpected test-list command")
        listed = check_step(suite["list"], root, require_artifacts)
        discovered = {line[:-6] for line in listed.splitlines() if line.endswith(": test") and not line[:-6].endswith("_worker")}
        required_tests = {case["test"] for case in spec["cases"]}
        need((required_tests <= discovered if spec.get("enumeration") == "selected" else discovered == required_tests),
             f"{label}: missing test or unreviewed native case")
        by_suite[suite["id"]] = (suite, spec)
        expected_results.update((suite["id"], case["test"], backend) for case in spec["cases"] for backend in case["backends"])
    keys = [(row.get("suite"), row.get("test"), row.get("backend")) for row in results]
    need(len(keys) == len(set(keys)) and set(keys) == expected_results, f"{label}: missing or wrong backend/native test result")
    for row in results:
        suite, spec = by_suite[row["suite"]]
        case = next(case for case in spec["cases"] if case["test"] == row["test"])
        need(row.get("roles") == case["roles"], f"{label}: native role outside reviewed case scope")
        need(row.get("source_sha256") == suite["source_sha256"] and row.get("executable_sha256") == suite["executable"]["sha256"],
             f"{label}: native source/executable binding mismatch")
        need(not ({"reference", "reference_trace_sha256", "adapter_trace_sha256"} & set(row)),
             f"{label}: fabricated native differential identity")
        captured_executable = captured_root / relative_artifact(suite["executable"]["path"])
        need(row["run"]["command"] == [str(captured_executable), "--exact", case["test"], "--nocapture", "--test-threads=1"],
             f"{label}: unexpected native test command")
        output = check_step(row["run"], root, require_artifacts)
        need(re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored;", output), f"{label}: test ignored, missing or failed")
        if case["markers"]:
            actual = {(test, backend) for test, backend in MARKER.findall(output)}
            expected = {(case["test"], backend) for backend in case.get("marker_backends", case["backends"])}
            need(actual == expected, f"{label}: missing/wrong native CASE_PASS backend marker")
        if spec.get("requires_adapter"):
            need(row["run"].get("environment", {}).get("ULMDB_COORDINATION_DYLIB") == str(captured_adapter),
                 f"{label}: actual shared-library input not selected")
            need(re.findall(r"(?<!\S)mixed-parent-provider=([^\r\n]*)", output) == [str(captured_adapter)],
                 f"{label}: missing actual mixed shared-library provider marker")
            need("mixed-parent-provider=direct" not in output, f"{label}: direct C implementation cannot qualify cross-copy integration")
    ordered_steps = []
    for spec in policy["suites"]:
        suite, _ = by_suite[spec["id"]]
        ordered_steps.extend((suite["build"], suite["list"]))
        for case in spec["cases"]:
            ordered_steps.append(next(row["run"] for row in results
                                      if row["suite"] == spec["id"] and row["test"] == case["test"]))
    need(receipt.get("steps") == ordered_steps, f"{label}: native command history differs from case results")
    return policy
